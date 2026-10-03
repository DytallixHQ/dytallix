//! Root-authorized release selection followed by owned process startup.
use crate::{
    config::{NativeServiceConfig, NodeRole},
    lease::LifecycleLease,
    processes::{with_owned_cleanup, ProcessOwner, Role},
    readiness::{self, AdapterReady, EngineReady},
};
use anyhow::{ensure, Context, Result};
use dytallix_fast_node::{
    consensus_settlement::{ConsensusApplication, ConsensusConfig, VerifiedReleaseAuthority},
    emergency_verifier::EmergencyVerifierConfig,
    root_genesis::{RootBootstrap, RootHelper},
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
    channel_readiness: Option<AdapterReady>,
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
        let candidate: DevelopmentCandidateV2Input =
            serde_json::from_slice(&config.candidate_config.read()?)?;
        let (authority, catalog, root) = authorize(&config, &candidate)?;
        admission.bind_catalog(&catalog)?;
        supervisor_security.check_current()?;
        // One production catalog serves every role and carries the adapter;
        // only endpoints start it (A5).
        let profile = &catalog.candidate().manifest().service_profile;
        ensure!(
            profile
                == if config.production() {
                    catalog::PRODUCTION_NATIVE_PROFILE
                } else if config.adapter_listen.is_some() {
                    catalog::DEVELOPMENT_NATIVE_HTTP_PROFILE
                } else {
                    catalog::DEVELOPMENT_NATIVE_PROFILE
                },
            "Service profile differs from the build and adapter selection"
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
            channel_readiness: None,
            started: false,
        })
    }
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.owner.cancellation_handle()
    }
    pub fn monitor_interval(&self) -> Duration {
        Duration::from_millis(self.config.monitor_interval_millis)
    }
    /// The production mode observes each long-running child once, at
    /// startup, then relies on kernel limits (P01, 30 September and 1
    /// October 2026); the development mode pauses them at every tick.
    fn check_children(&mut self, context: &'static str) -> Result<()> {
        if self.config.production() {
            self.owner.check_security().context(context)
        } else {
            self.owner.observe_all().map(drop).context(context)
        }
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
        self.owner.start_bridge_with(
            &self.config.bridge_socket(),
            &bridge_arguments(&self.config),
        )?;
        self.owner.start_engine(
            &engine_arguments(&self.config),
            &self.config.rpc_socket(),
            &self.config.operator_rpc_socket(),
        )?;
        let engine_pid = self
            .owner
            .owned_pids()
            .into_iter()
            .find(|(role, _)| *role == Role::Engine)
            .context("Owned engine handle missing")?
            .1;
        // A production node may first catch up by state sync and block sync;
        // readiness then waits up to its catch-up budget, rechecking the
        // children, locks and the supervisor's own security as it waits.
        let catch_up = self
            .config
            .catch_up_millis
            .map(std::time::Duration::from_millis);
        let (owner, lease, security) = (&mut self.owner, &self.lease, &self.supervisor_security);
        self.engine_readiness = Some(crate::startup_diagnostic::phase(readiness::verify_engine(
            &self.config.rpc_socket(),
            engine_pid,
            &self.authority.expected_candidate().chain_id,
            self.authority.committed_info(),
            &self.config.process,
            catch_up,
            || {
                if catch_up.is_some() {
                    lease.recheck()?;
                    security.check_current()?;
                }
                owner.check_alive()
            },
        ), Role::Engine, crate::startup_diagnostic::Stage::EngineReadiness)?);
        if let Some(listen) = &self.config.adapter_listen {
            let mut args: Vec<std::ffi::OsString> = vec![
                "--profile".into(),
                ADAPTER_PROFILE.into(),
                "--home".into(),
                self.config.home.as_os_str().into(),
                "--listen".into(),
                listen.into(),
            ];
            if let Some(limits) = &self.config.adapter_limits {
                args.extend(limits.args());
            }
            if let Some(status) = &self.config.adapter_status_listen {
                args.extend(["--status-listen".into(), status.into()]);
            }
            let chain_id = self.authority.expected_candidate().chain_id.clone();
            let channel = match &self.config.adapter_channel {
                Some(channel) => {
                    let pin = channel.endpoint_pin()?;
                    ensure!(
                        pin.network == chain_id,
                        "Channel pin names another network than the chain"
                    );
                    args.extend(channel.args(&chain_id));
                    Some((channel.address()?, pin))
                }
                None => None,
            };
            self.owner.start_adapter(&args)?;
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
            if let Some((address, pin)) = channel {
                self.channel_readiness = Some(crate::startup_diagnostic::phase(readiness::verify_adapter_channel(
                    address,
                    &pin,
                    adapter_pid,
                    &chain_id,
                    self.engine_readiness
                        .as_ref()
                        .context("Engine readiness absent")?
                        .minimum_height(),
                    &self.config.process,
                    || self.owner.check_alive(),
                ), Role::Adapter, crate::startup_diagnostic::Stage::AdapterChannel)?);
            }
        }
        self.lease.recheck()?;
        self.supervisor_security.check_current()?;
        self.self_snapshot = observation::observe_current_process(
            "service_supervisor",
            &self.catalog,
            &self.observation_bounds,
        )?;
        self.supervisor_security.check_current()?;
        self.check_children("Native observation call=startup_final_children")?;
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
            self.check_children("Native observation call=monitor_children")?;
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
    /// The report's scope and mode fields. Qualification belongs to the
    /// accepted release (E06, T03), so no report claims it.
    fn scope(&self, kind: &str) -> Value {
        if self.config.production() {
            json!({"scope":format!("PRODUCTION_NATIVE_SERVICE_{kind}"),"mode":self.config.mode,
                "role":self.config.role.map(NodeRole::as_str)})
        } else {
            json!({"scope":format!("DEVELOPMENT_NATIVE_SERVICE_{kind}"),"production_qualified":false})
        }
    }
    fn with_scope(&self, kind: &str, mut report: Value) -> Value {
        if let (Some(report), Some(scope)) = (report.as_object_mut(), self.scope(kind).as_object()) {
            report.extend(scope.clone());
        }
        report
    }
    pub fn final_timing_report(&self) -> Value {
        self.with_scope("FINAL_TIMINGS", json!({
            "supervisor_pid":std::process::id(),
            "release_manifest_sha512":self.authority.expected_candidate().manifest_sha512,
            "observation_timings":self.owner.observation_timings(),
            "first_failure_children":self.owner.first_failure_snapshot(),
            "helper_admission":dytallix_fast_node::root_genesis::helper_admission_receipt(),
            "application_startup_helper_admission_receipt":self.application_startup_helper_admission_receipt}))
    }
    pub fn report(&self) -> Value {
        self.with_scope("SNAPSHOTS", json!({
            "release_manifest_sha512":self.authority.expected_candidate().manifest_sha512,
            "preflight_height":self.authority.committed_info().map(|v|v.height),
            "started":self.started,"supervisor":self.self_snapshot.report(),
            "engine_readiness":self.engine_readiness.as_ref().map(|v|json!({"chain_id":v.chain_id(),"block_height":v.block_height(),"application_height":v.application_info().height,"application_hash":v.application_info().app_hash,"pid":v.engine_pid()})),
            "adapter_readiness":self.adapter_readiness.as_ref().map(|v|json!({"chain_id":v.chain_id(),"block_height":v.block_height(),"pid":v.adapter_pid(),"listener_identity_bound":v.listener_identity_bound()})),
            "channel_readiness":self.channel_readiness.as_ref().map(|v|json!({"chain_id":v.chain_id(),"block_height":v.block_height(),"pid":v.adapter_pid(),"listener_identity_bound":v.listener_identity_bound()})),
            "observation_timings":self.owner.observation_timings(),
            "helper_admission":dytallix_fast_node::root_genesis::helper_admission_receipt(),
            "application_startup_helper_admission_receipt":self.application_startup_helper_admission_receipt,
            "children":self.owner.snapshots().iter().map(|v|v.report()).collect::<Vec<_>>()}))
    }
}

/// Verify the root authority and the release catalog it selects. A
/// production build has only the threshold root (A2, A4): three of the five
/// genesis signatures over the exact genesis files and release.
#[cfg(feature = "production")]
fn authorize(
    config: &NativeServiceConfig,
    candidate: &DevelopmentCandidateV2Input,
) -> Result<(VerifiedReleaseAuthority, catalog::VerifiedMemberFiles, RootHelper)> {
    let (consensus_source, consensus, genesis, emergency, restart) = inputs(config)?;
    let root = dytallix_fast_node::root_genesis::RootGenesis::from_config(&config.root_config.path)?;
    let authority = ConsensusApplication::preflight_release_with_root(
        config.database(),
        &consensus,
        &genesis,
        &consensus_source,
        &root,
        restart.as_deref(),
    )?;
    let catalog = verify_catalog(candidate, &authority, &root, &emergency)?;
    Ok((authority, catalog, root.bootstrap_helper()?))
}

/// The development build's single-key development root.
#[cfg(not(feature = "production"))]
fn authorize(
    config: &NativeServiceConfig,
    candidate: &DevelopmentCandidateV2Input,
) -> Result<(VerifiedReleaseAuthority, catalog::VerifiedMemberFiles, RootHelper)> {
    let (consensus_source, consensus, genesis, emergency, restart) = inputs(config)?;
    let root = dytallix_fast_node::root_genesis::DevelopmentRootGenesis::from_development_config(
        &config.root_config.path,
    )?;
    let authority = ConsensusApplication::preflight_development_release_with_restart(
        config.database(),
        &consensus,
        &genesis,
        &consensus_source,
        &root,
        restart.as_deref(),
    )?;
    let catalog = verify_catalog(candidate, &authority, &root, &emergency)?;
    Ok((authority, catalog, root.bootstrap_helper()?))
}

type Inputs = (Vec<u8>, ConsensusConfig, Vec<u8>, EmergencyVerifierConfig, Option<Vec<u8>>);

/// The pinned application inputs, read under both lifecycle locks.
fn inputs(config: &NativeServiceConfig) -> Result<Inputs> {
    let consensus_source = config.consensus_config.read()?;
    let consensus: ConsensusConfig = serde_json::from_slice(&consensus_source)?;
    let genesis = config.application_genesis.read()?;
    let emergency: EmergencyVerifierConfig =
        serde_json::from_slice(&config.emergency_verifier_config.read()?)?;
    let restart = config
        .restart_authorization
        .as_ref()
        .map(|pin| pin.read())
        .transpose()?;
    Ok((consensus_source, consensus, genesis, emergency, restart))
}

fn application_arguments(config: &NativeServiceConfig) -> Vec<OsString> {
    // A production build opens only through the threshold root and its
    // controls (A2, A4); it has no development flags.
    let [root, verifier, candidate] = if config.production() {
        ["--root-config", "--verifier-config", "--candidate-config"]
    } else {
        [
            "--development-root-config",
            "--development-emergency-verifier-config",
            "--development-candidate-config",
        ]
    };
    let mut args: Vec<(&str, OsString)> = vec![
        ("--config", config.consensus_config.path.clone().into()),
        ("--genesis", config.application_genesis.path.clone().into()),
        ("--db", config.database().into()),
        (root, config.root_config.path.clone().into()),
        (verifier, config.emergency_verifier_config.path.clone().into()),
        (candidate, config.candidate_config.path.clone().into()),
        ("--block-history", config.block_history.arg().into()),
        ("--metrics-dir", config.metrics.directory.clone().into()),
        (
            "--metrics-interval-seconds",
            config.metrics.interval_seconds.to_string().into(),
        ),
    ];
    if let Some(restart) = &config.restart_authorization {
        args.push(("--restart-authorization", restart.path.clone().into()));
    }
    if let Some(snapshots) = &config.snapshots {
        args.extend([
            ("--snapshot-dir", snapshots.directory.clone().into()),
            (
                "--snapshot-interval",
                snapshots.interval_blocks.to_string().into(),
            ),
            ("--snapshot-keep", snapshots.keep.to_string().into()),
        ]);
    }
    args.into_iter()
        .flat_map(|(key, value)| [OsString::from(key), value])
        .collect()
}

/// The bridge serves the application's snapshots (state sync v1).
fn bridge_arguments(config: &NativeServiceConfig) -> Vec<OsString> {
    match &config.snapshots {
        Some(snapshots) => vec!["--snapshot-dir".into(), snapshots.directory.clone().into()],
        None => Vec::new(),
    }
}

/// The production transport profile (A4), the only profile of a production
/// engine build.
const PRODUCTION_TRANSPORT_PROFILE: &str = "dytallix-pqc-production-v1";

/// The HTTP adapter profile of this build: an adapter build serves only its
/// own profile (production activation v1).
#[cfg(not(feature = "production"))]
const ADAPTER_PROFILE: &str = "dytallix-pqc-http-local-v1";
#[cfg(feature = "production")]
const ADAPTER_PROFILE: &str = "dytallix-pqc-http-production-v1";

fn engine_arguments(config: &NativeServiceConfig) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "start".into(),
        "--home".into(),
        config.home.as_os_str().into(),
        "--p2p-profile".into(),
        if config.production() {
            PRODUCTION_TRANSPORT_PROFILE
        } else {
            "dytallix-pqc-loopback-v1"
        }
        .into(),
        "--rpc-profile".into(),
        "dytallix-pqc-unix-v1".into(),
        "--metrics-dir".into(),
        config.metrics.directory.clone().into(),
        "--metrics-interval".into(),
        format!("{}s", config.metrics.interval_seconds).into(),
    ];
    // The engine checks the host's binding again at start, including the
    // seed-derived peer key.
    if let Some(binding) = &config.binding {
        args.extend(["--binding".into(), binding.path.clone().into()]);
    }
    for export in config.state_sync.iter().flat_map(|s| &s.light_blocks) {
        args.extend(["--light-blocks".into(), export.clone().into()]);
    }
    args
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
mod argument_tests {
    use super::*;
    fn config(snapshots: Value) -> NativeServiceConfig {
        let pin = |path: &str| json!({"path":path,"sha256":"0".repeat(64),"max_bytes":1});
        serde_json::from_value(json!({
            "schema":1,"mode":"disposable-loopback-native","home":"/n","lock_directory":"/l",
            "consensus_config":pin("/c"),"application_genesis":pin("/g"),"root_config":pin("/r"),
            "root_public_inputs":[],"emergency_verifier_config":pin("/e"),
            "candidate_config":pin("/k"),"process_admission":pin("/a"),"engine_inputs":[],
            "validator_public_key_sha256":"0".repeat(64),
            "process":{"startup_millis":1,"stop_millis":1,"kill_millis":1,"poll_millis":1,
                "max_argument_bytes":1,"max_environment_bytes":1,"max_probe_request_bytes":1,
                "max_probe_response_bytes":1},
            "environment":{},"monitor_interval_millis":1,"max_state_entries":1,
            "metrics":{"directory":"/m","interval_seconds":15},"snapshots":snapshots,
            "block_history":"archive"}))
        .unwrap()
    }
    #[test]
    fn a_pinned_restart_reaches_the_application() {
        let mut config = config(Value::Null);
        assert!(!strings(application_arguments(&config)).iter().any(|a| a == "--restart-authorization"));
        config.restart_authorization = Some(
            serde_json::from_value(json!({"path":"/r","sha256":"0".repeat(64),"max_bytes":1})).unwrap(),
        );
        let app = strings(application_arguments(&config));
        assert!(app.windows(2).any(|w| w == ["--restart-authorization", "/r"]));
    }
    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|v| v.into_string().unwrap()).collect()
    }
    #[test]
    fn every_child_receives_its_outputs() {
        let config = config(json!({"directory":"/s","interval_blocks":100,"keep":2}));
        let app = strings(application_arguments(&config));
        for pair in [
            ["--block-history", "archive"],
            ["--metrics-dir", "/m"],
            ["--metrics-interval-seconds", "15"],
            ["--snapshot-dir", "/s"],
            ["--snapshot-interval", "100"],
            ["--snapshot-keep", "2"],
        ] {
            assert!(app.windows(2).any(|w| w == pair), "{pair:?}");
        }
        assert_eq!(strings(bridge_arguments(&config)), ["--snapshot-dir", "/s"]);
        let engine = strings(engine_arguments(&config));
        assert!(engine.ends_with(&[
            "--metrics-dir".into(),
            "/m".into(),
            "--metrics-interval".into(),
            "15s".into()
        ]));
    }
    #[test]
    fn production_mode_uses_the_root_controls_binding_and_light_blocks() {
        let mut production = config(Value::Null);
        production.mode = crate::config::PRODUCTION_MODE.into();
        production.role = Some(NodeRole::Sentry);
        production.binding = Some(
            serde_json::from_value(json!({"path":"/b","sha256":"0".repeat(64),"max_bytes":1})).unwrap(),
        );
        production.state_sync = Some(
            serde_json::from_value(json!({"light_blocks":["/lb/primary","/lb/witness"]})).unwrap(),
        );
        let app = strings(application_arguments(&production));
        for pair in [["--root-config", "/r"], ["--verifier-config", "/e"], ["--candidate-config", "/k"]] {
            assert!(app.windows(2).any(|w| w == pair), "{pair:?}");
        }
        assert!(!app.iter().any(|a| a.starts_with("--development")));
        let engine = strings(engine_arguments(&production));
        assert!(engine.windows(2).any(|w| w == ["--p2p-profile", PRODUCTION_TRANSPORT_PROFILE]));
        assert!(engine.ends_with(&[
            "--binding".into(),
            "/b".into(),
            "--light-blocks".into(),
            "/lb/primary".into(),
            "--light-blocks".into(),
            "/lb/witness".into(),
        ]));
        // The development mode keeps its flags and passes neither.
        let config = config(Value::Null);
        let engine = strings(engine_arguments(&config));
        assert!(engine.windows(2).any(|w| w == ["--p2p-profile", "dytallix-pqc-loopback-v1"]));
        assert!(!engine.iter().any(|a| a == "--binding" || a == "--light-blocks"));
        assert!(strings(application_arguments(&config)).iter().any(|a| a == "--development-root-config"));
    }
    #[test]
    fn snapshots_are_optional() {
        let config = config(Value::Null);
        assert!(!strings(application_arguments(&config))
            .iter()
            .any(|a| a.starts_with("--snapshot")));
        assert!(bridge_arguments(&config).is_empty());
    }
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
