//! Root-authorized release selection followed by owned process startup.
use crate::{
    config::NativeServiceConfig,
    lease::LifecycleLease,
    processes::{with_owned_cleanup, ProcessOwner, Role},
    readiness::{self, AdapterReady, EngineReady},
};
use anyhow::{ensure, Context, Result};
use dytallix_fast_node::{
    consensus_settlement::{ConsensusApplication, ConsensusConfig, VerifiedReleaseAuthority},
    emergency_verifier::EmergencyVerifierConfig,
    root_genesis::DevelopmentRootGenesis,
    runtime_candidate_v2::{verify_catalog, DevelopmentCandidateV2Input},
};
use dytallix_release_runtime::{
    component_candidate as catalog, observation, ownership_security::SecurityState,
};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::Path;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;

pub struct NativeService {
    // Drop the process owner before the lease if a caller leaves without stop().
    owner: ProcessOwner,
    lease: LifecycleLease,
    config: NativeServiceConfig,
    authority: VerifiedReleaseAuthority,
    catalog: catalog::VerifiedMemberFiles,
    observation_bounds: observation::Bounds,
    self_snapshot: observation::Snapshot,
    supervisor_security: SecurityState,
    engine_readiness: Option<EngineReady>,
    application_startup_helper_admission_receipt: Option<Value>,
    application_helper_expectation: Value,
    adapter_readiness: Option<AdapterReady>,
    started: bool,
}

impl NativeService {
    pub fn prepare(path: &Path) -> Result<Self> {
        ensure!(cfg!(target_os = "linux"), "Native service requires Linux");
        let config = NativeServiceConfig::load(path)?;
        let admission = config.admission()?;
        let supervisor_security = admission.supervisor_state()?;
        let (service_lock, signer_lock) = config.lock_paths();
        let lease = LifecycleLease::acquire(&service_lock, &signer_lock)?;
        // Re-read pinned settings with both lifecycle locks held.
        config.validate()?;
        let consensus_source = config.consensus_config.read()?;
        let consensus: ConsensusConfig = serde_json::from_slice(&consensus_source)?;
        let genesis = config.application_genesis.read()?;
        let root = DevelopmentRootGenesis::from_development_config(&config.root_config.path)?;
        let emergency: EmergencyVerifierConfig =
            serde_json::from_slice(&config.emergency_verifier_config.read()?)?;
        let candidate: DevelopmentCandidateV2Input =
            serde_json::from_slice(&config.candidate_config.read()?)?;
        let authority = ConsensusApplication::preflight_development_release_from_root(
            config.database(),
            &consensus,
            &genesis,
            &consensus_source,
            &root,
        )?;
        let catalog = verify_catalog(&candidate, &authority, &root, &emergency)?;
        admission.bind_catalog(&catalog)?;
        supervisor_security.check_current()?;
        let profile = &catalog.candidate().manifest().service_profile;
        ensure!(
            profile
                == if config.adapter_listen.is_some() {
                    catalog::DEVELOPMENT_NATIVE_HTTP_PROFILE
                } else {
                    catalog::DEVELOPMENT_NATIVE_PROFILE
                },
            "Service roles differ from configured adapter selection"
        );
        let observation_bounds = candidate.observation_bounds()?;
        let self_snapshot = observation::observe_current_process(
            "service_supervisor",
            &catalog,
            &observation_bounds,
        )
        .context("Native observation phase=prepare")?;
        // Root verification may execute trusted helper code. Recheck public
        // inputs and lock identities before allowing writable child startup.
        config.validate()?;
        lease.recheck()?;
        let mut owner = ProcessOwner::new(
            catalog.clone(),
            observation_bounds.clone(),
            config.process.bounds()?,
            config.environment.clone(),
        )?;
        let mut application_security = supervisor_security.clone();
        application_security.apparmor_label = admission.application_owner_label.clone();
        let mut workload_security = supervisor_security.clone();
        workload_security.apparmor_label = admission.workload_label.clone();
        owner.set_admission_security(application_security, workload_security)?;
        let pause = config
            .observation_pause
            .as_ref()
            .context("Explicit observation pause budget required")?;
        owner.set_observation_pause(
            Duration::from_millis(pause.total_millis),
            Duration::from_millis(pause.cleanup_reserve_millis),
        )?;
        let (service_file, signer_file) = lease.files();
        owner.attach_lifecycle_leases(service_file, signer_file)?;
        let helper_policy = root.helper_execution.as_ref()
            .context("Observed root helper policy missing")?;
        helper_policy.owner_security.validate()?;
        ensure!(helper_policy.owner_security.supervisor_label == admission.supervisor_label
            && helper_policy.owner_security.application_owner_label == admission.application_owner_label
            && helper_policy.owner_security.workload_label == admission.workload_label
            && helper_policy.owner_security.helper_label == admission.helper_label
            && helper_policy.owner_security.uid == admission.uid
            && helper_policy.owner_security.gid == admission.gid,
            "Root helper roles differ from service admission");
        let genesis_helper = catalog.role_file("genesis_bootstrap_verifier")
            .context("Genesis helper missing from verified catalog")?;
        let control_helper = catalog.role_file("control_verifier")
            .context("Control helper missing from verified catalog")?;
        ensure!(genesis_helper.id() == control_helper.id()
            && genesis_helper.path() == control_helper.path()
            && genesis_helper.path() == root.helper_path
            && genesis_helper.digest().sha256 == root.helper_sha256
            && genesis_helper.digest().sha512 == helper_policy.helper_sha512
            && genesis_helper.digest().bytes == helper_policy.helper_bytes,
            "Root helper file differs from verified service candidate");
        let application_helper_expectation = json!({
            "helper_sha512":helper_policy.helper_sha512,
            "helper_bytes":helper_policy.helper_bytes,
            "uid":helper_policy.owner_security.uid,
            "gid":helper_policy.owner_security.gid,
            "owner_label":helper_policy.owner_security.application_owner_label,
            "helper_label":helper_policy.owner_security.helper_label,
            "mount_namespace":supervisor_security.mount_namespace,
        });
        Ok(Self {
            owner,
            lease,
            config,
            authority,
            catalog,
            observation_bounds,
            self_snapshot,
            supervisor_security,
            engine_readiness: None,
            application_startup_helper_admission_receipt: None,
            application_helper_expectation,
            adapter_readiness: None,
            started: false,
        })
    }
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.owner.cancellation_handle()
    }
    pub fn monitor_interval(&self) -> Duration {
        Duration::from_millis(self.config.monitor_interval_millis)
    }
    pub fn start(&mut self) -> Result<()> {
        ensure!(!self.started, "Native service already started");
        let result = self.start_inner().context("Native service phase=startup");
        if let Err(error) = result {
            self.owner.capture_failure();
            return with_owned_cleanup(Err(error), self.owner.shutdown(), "Startup");
        }
        self.started = true;
        Ok(())
    }
    fn start_inner(&mut self) -> Result<()> {
        self.config.validate()?;
        self.config.admission()?.require_enforced_profiles()?;
        self.lease.recheck()?;
        self.supervisor_security.check_current()?;
        self.self_snapshot = observation::observe_current_process(
            "service_supervisor",
            &self.catalog,
            &self.observation_bounds,
        )?;
        self.supervisor_security.check_current()?;
        let args = application_arguments(&self.config);
        self.owner.start_application(&args)?;
        let response = self.owner.exchange_application(
            b"{\"method\":\"info\",\"payload\":{}}\n",
            self.config.process.max_probe_response_bytes,
            Duration::from_millis(self.config.process.startup_millis),
        )?;
        verify_info(&response, &self.authority)?;
        let application_pid = self.owner.owned_pids().into_iter()
            .find(|(role, _)| *role == Role::Application)
            .context("Owned application handle missing")?.1;
        self.application_startup_helper_admission_receipt = Some(application_helper_receipt(
            &response, application_pid, &self.application_helper_expectation)?);
        self.owner
            .observe_application()
            .context("Native observation call=startup_after_application_info")?;
        self.owner.start_bridge(&self.config.bridge_socket())?;
        self.owner.start_engine(
            &[
                "start".into(),
                "--home".into(),
                self.config.home.as_os_str().into(),
                "--p2p-profile".into(),
                "dytallix-pqc-loopback-v1".into(),
                "--rpc-profile".into(),
                "dytallix-pqc-unix-v1".into(),
            ],
            &self.config.rpc_socket(),
        )?;
        let engine_pid = self
            .owner
            .owned_pids()
            .into_iter()
            .find(|(role, _)| *role == Role::Engine)
            .context("Owned engine handle missing")?
            .1;
        self.engine_readiness = Some(crate::startup_diagnostic::phase(readiness::verify_engine(
            &self.config.rpc_socket(),
            engine_pid,
            &self.authority.expected_candidate().chain_id,
            self.authority.committed_info(),
            &self.config.process,
            || self.owner.check_alive(),
        ), Role::Engine, crate::startup_diagnostic::Stage::EngineReadiness)?);
        if let Some(listen) = &self.config.adapter_listen {
            self.owner.start_adapter(&[
                "--profile".into(),
                "dytallix-pqc-http-local-v1".into(),
                "--home".into(),
                self.config.home.as_os_str().into(),
                "--listen".into(),
                listen.into(),
            ])?;
            let adapter_pid = self
                .owner
                .owned_pids()
                .into_iter()
                .find(|(role, _)| *role == Role::Adapter)
                .context("Owned adapter handle missing")?
                .1;
            self.adapter_readiness = Some(crate::startup_diagnostic::phase(readiness::verify_adapter(
                listen.parse()?,
                adapter_pid,
                &self.authority.expected_candidate().chain_id,
                self.engine_readiness
                    .as_ref()
                    .context("Engine readiness absent")?
                    .minimum_height(),
                &self.config.process,
                || self.owner.check_alive(),
            ), Role::Adapter, crate::startup_diagnostic::Stage::AdapterReadiness)?);
        }
        self.lease.recheck()?;
        self.supervisor_security.check_current()?;
        self.self_snapshot = observation::observe_current_process(
            "service_supervisor",
            &self.catalog,
            &self.observation_bounds,
        )?;
        self.supervisor_security.check_current()?;
        self.owner
            .observe_all()
            .context("Native observation call=startup_final_children")?;
        self.owner.check_alive()?;
        Ok(())
    }
    pub fn monitor_tick(&mut self) -> Result<()> {
        ensure!(self.started, "Native service has not started");
        let result: Result<()> = (|| {
            self.lease.recheck()?;
            self.owner.check_alive()?;
            self.supervisor_security.check_current()?;
            self.self_snapshot = observation::observe_current_process(
                "service_supervisor",
                &self.catalog,
                &self.observation_bounds,
            )?;
            self.supervisor_security.check_current()?;
            self.owner
                .observe_all()
                .context("Native observation call=monitor_children")?;
            Ok(())
        })();
        if let Err(error) = result.context("Native service phase=monitor_tick") {
            self.owner.capture_failure();
            return with_owned_cleanup(Err(error), self.owner.shutdown(), "Monitoring");
        }
        Ok(())
    }
    pub fn capture_failure(&mut self) { self.owner.capture_failure(); }
    pub fn stop(&mut self) -> Result<()> {
        self.owner.shutdown()
    }
    pub fn observation_timings(&self) -> Value { self.owner.observation_timings() }
    pub fn final_timing_report(&self) -> Value {
        json!({"scope":"DEVELOPMENT_NATIVE_SERVICE_FINAL_TIMINGS",
            "production_qualified":false,
            "supervisor_pid":std::process::id(),
            "release_manifest_sha512":self.authority.expected_candidate().manifest_sha512,
            "observation_timings":self.owner.observation_timings(),
            "first_failure_children":self.owner.first_failure_snapshot(),
            "helper_admission":dytallix_fast_node::root_genesis::helper_admission_receipt(),
            "application_startup_helper_admission_receipt":self.application_startup_helper_admission_receipt})
    }
    pub fn report(&self) -> Value {
        json!({"scope":"DEVELOPMENT_NATIVE_SERVICE_SNAPSHOTS", "production_qualified":false,
            "release_manifest_sha512":self.authority.expected_candidate().manifest_sha512,
            "preflight_height":self.authority.committed_info().map(|v|v.height),
            "started":self.started,"supervisor":self.self_snapshot.report(),
            "engine_readiness":self.engine_readiness.as_ref().map(|v|json!({"chain_id":v.chain_id(),"block_height":v.block_height(),"application_height":v.application_info().height,"application_hash":v.application_info().app_hash,"pid":v.engine_pid()})),
            "adapter_readiness":self.adapter_readiness.as_ref().map(|v|json!({"chain_id":v.chain_id(),"block_height":v.block_height(),"pid":v.adapter_pid(),"listener_identity_bound":v.listener_identity_bound()})),
            "observation_timings":self.owner.observation_timings(),
            "helper_admission":dytallix_fast_node::root_genesis::helper_admission_receipt(),
            "application_startup_helper_admission_receipt":self.application_startup_helper_admission_receipt,
            "children":self.owner.snapshots().iter().map(|v|v.report()).collect::<Vec<_>>()})
    }
}

fn application_arguments(config: &NativeServiceConfig) -> Vec<OsString> {
    [
        ("--config", config.consensus_config.path.clone()),
        ("--genesis", config.application_genesis.path.clone()),
        ("--db", config.database()),
        ("--development-root-config", config.root_config.path.clone()),
        (
            "--development-emergency-verifier-config",
            config.emergency_verifier_config.path.clone(),
        ),
        (
            "--development-candidate-config",
            config.candidate_config.path.clone(),
        ),
    ]
    .into_iter()
    .flat_map(|(key, value)| [OsString::from(key), value.into_os_string()])
    .collect()
}

fn verify_info(bytes: &[u8], authority: &VerifiedReleaseAuthority) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(
        value.get("ok").and_then(Value::as_bool) == Some(true),
        "Application readiness probe failed"
    );
    let info = value
        .get("result")
        .context("Application info result missing")?;
    let height = info
        .get("height")
        .and_then(Value::as_u64)
        .context("Application height missing")?;
    if let Some(expected) = authority.committed_info() {
        ensure!(
            height == expected.height
                && info.get("app_hash").and_then(Value::as_str) == Some(expected.app_hash.as_str()),
            "Application state differs from verified preflight"
        );
    } else {
        ensure!(height == 0, "Fresh application reported a committed height");
    }
    Ok(())
}

// Retain bounded diagnostics from the already-owned application response.
// This receipt grants no release authority and does not enter consensus state.
fn application_helper_receipt(bytes: &[u8], application_pid: u32, expected: &Value) -> Result<Value> {
    let response: Value = serde_json::from_slice(bytes)?;
    let receipt = response.get("result")
        .and_then(|value| value.get("helper_admission_receipt"))
        .context("Application helper admission receipt missing")?;
    ensure!(receipt.is_object()
        && receipt.get("scope").and_then(Value::as_str)
            == Some("LOCAL_COMPLETED_HELPER_ADMISSION_V1")
        && receipt.get("owner_pid").and_then(Value::as_u64) == Some(u64::from(application_pid))
        && receipt.get("owner_tid").and_then(Value::as_u64) == Some(u64::from(application_pid)),
        "Application helper receipt creator differs from owned application");
    let required = json!({"schema":1,"production_qualified":false,"release_authority":false,
        "status":"VERIFIED","natural_exit_code":0,"no_new_privileges":1,"seccomp":2,
        "guard_ready_before_observation":true,"go_after_owner_admission":true,
        "protocol_ready_after_go":true,"ack_after_observation":true,
        "same_owned_child":true,"reaped":true});
    for (key, value) in required.as_object().unwrap().iter()
        .chain(expected.as_object().context("Trusted helper expectation missing")?.iter()) {
        ensure!(receipt.get(key) == Some(value), "Application helper receipt field differs: {key}");
    }
    ensure!(serde_json::to_vec(receipt)?.len() <= 4096,
        "Application helper receipt exceeds fixed bound");
    Ok(receipt.clone())
}

#[cfg(test)]
mod helper_receipt_tests {
    use super::*;
    fn response(owner: u32) -> Value {
        json!({"result":{"helper_admission_receipt":{
            "scope":"LOCAL_COMPLETED_HELPER_ADMISSION_V1",
            "owner_pid":owner,"owner_tid":owner,
            "schema":1,"production_qualified":false,"release_authority":false,
            "status":"VERIFIED","natural_exit_code":0,"no_new_privileges":1,"seccomp":2,
            "guard_ready_before_observation":true,"go_after_owner_admission":true,
            "protocol_ready_after_go":true,"ack_after_observation":true,
            "same_owned_child":true,"reaped":true}}})
    }
    #[test]
    fn binds_receipt_to_owned_application() {
        let raw = serde_json::to_vec(&response(123)).unwrap();
        assert!(application_helper_receipt(&raw, 123, &json!({})).is_ok());
        assert!(application_helper_receipt(&raw, 124, &json!({})).is_err());
    }
    #[test]
    fn refuses_missing_wrong_scope_and_worker_thread_receipts() {
        assert!(application_helper_receipt(b"{}", 123, &json!({})).is_err());
        let mut wrong = response(123);
        wrong["result"]["helper_admission_receipt"]["scope"] = json!("OTHER");
        assert!(application_helper_receipt(&serde_json::to_vec(&wrong).unwrap(), 123, &json!({})).is_err());
        let mut worker = response(123);
        worker["result"]["helper_admission_receipt"]["owner_tid"] = json!(124);
        assert!(application_helper_receipt(&serde_json::to_vec(&worker).unwrap(), 123, &json!({})).is_err());
    }
    #[test]
    fn refuses_oversized_diagnostic() {
        let mut oversized = response(123);
        oversized["result"]["helper_admission_receipt"]["extra"] = json!("x".repeat(4096));
        assert!(application_helper_receipt(&serde_json::to_vec(&oversized).unwrap(), 123, &json!({})).is_err());
    }
    #[test]
    fn refuses_failed_lifecycle_and_mismatched_trusted_helper() {
        let mut value = response(123);
        value["result"]["helper_admission_receipt"]["reaped"] = json!(false);
        assert!(application_helper_receipt(&serde_json::to_vec(&value).unwrap(), 123, &json!({})).is_err());
        let mut value = response(123);
        value["result"]["helper_admission_receipt"]["helper_sha512"] = json!("a");
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(application_helper_receipt(&raw, 123, &json!({"helper_sha512":"b"})).is_err());
        assert!(application_helper_receipt(&raw, 123, &json!({"helper_sha512":"a"})).is_ok());
    }

}
