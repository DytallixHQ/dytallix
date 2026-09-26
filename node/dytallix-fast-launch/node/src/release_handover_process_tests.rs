//! Real signed release handover through two distinct executable processes.
//! Production remains disabled. Artifact provenance is supplied by the build record.
use super::*;
use crate::release_handover as handover;
use serde_json::{json, Value};
use sha2::Sha512;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FRAME_LIMIT: u64 = 8 * 1024 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(180);

struct HandoverProcess {
    child: Child,
    requests: Option<mpsc::Sender<Value>>,
    responses: mpsc::Receiver<std::result::Result<Value, String>>,
    worker: Option<JoinHandle<()>>,
    stderr: PathBuf,
}
impl HandoverProcess {
    fn open_with_candidate(
        binary: &Path,
        root: &RootFixture,
        database: &Path,
        label: &str,
        candidate_config: Option<&Path>,
    ) -> Self {
        let dir = root._directory.path();
        let stderr = dir.join(format!("process-{label}.stderr"));
        let mut command = Command::new(binary);
        command
            .arg("--config").arg(dir.join("consensus.json"))
            .arg("--genesis").arg(dir.join("genesis.json"))
            .arg("--db").arg(database)
            .arg("--development-root-config").arg(dir.join("process-root.json"))
            .arg("--development-emergency-verifier-config").arg(dir.join("process-verifier.json"))
            .stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap());
        if let Some(path) = candidate_config {
            command.arg("--development-candidate-config").arg(path);
        }
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (requests, receiver) = mpsc::channel::<Value>();
        let (sender, responses) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            for request in receiver {
                let result = (|| -> std::result::Result<Value, String> {
                    let mut frame = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
                    frame.push(b'\n');
                    if frame.len() as u64 > FRAME_LIMIT { return Err("Oversized request".into()); }
                    input.write_all(&frame).map_err(|e| e.to_string())?;
                    input.flush().map_err(|e| e.to_string())?;
                    let mut line = Vec::new();
                    let count = (&mut output).take(FRAME_LIMIT + 1)
                        .read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
                    if count == 0 || count as u64 > FRAME_LIMIT || line.last() != Some(&b'\n') {
                        return Err("Missing or oversized process response".into());
                    }
                    serde_json::from_slice(&line).map_err(|e| e.to_string())
                })();
                let failed = result.is_err();
                if sender.send(result).is_err() || failed { break; }
            }
        });
        Self { child, requests: Some(requests), responses, worker: Some(worker), stderr }
    }
    fn raw(&mut self, method: &str, payload: Value) -> Value {
        self.requests.as_ref().unwrap().send(json!({"method":method,"payload":payload})).unwrap();
        match self.responses.recv_timeout(FRAME_TIMEOUT) {
            Ok(Ok(value)) => value,
            error => panic!("Process request {method} failed: {error:?}; stderr: {}",
                std::fs::read_to_string(&self.stderr).unwrap_or_default()),
        }
    }
    fn request(&mut self, method: &str, payload: Value) -> Value {
        let response = self.raw(method, payload);
        assert_eq!(response["ok"], true, "{method}: {response}");
        response["result"].clone()
    }
    fn rejected(&mut self, method: &str, payload: Value, expected_error: &str) {
        let response = self.raw(method, payload);
        assert_eq!(response["ok"], false, "{method}: {response}");
        assert!(response["error"].as_str().unwrap().contains(expected_error), "{response}");
    }
    fn info(&mut self) -> Info {
        let info = self.request("info", json!({}));
        Info { height: info["height"].as_u64().unwrap(), app_hash: info["app_hash"].as_str().unwrap().into() }
    }
    fn snapshot(&mut self) -> (Info, Value) { (self.info(), self.query("/status")) }
    fn commit_txs(&mut self, height: u64, txs: &[Vec<u8>]) -> Value {
        let input = process_block(height, txs);
        assert_eq!(self.request("process_proposal", input.clone())["accept"], true);
        let finalized = self.request("finalize_block", input);
        assert_eq!(finalized["tx_results"].as_array().unwrap().len(), txs.len());
        for result in finalized["tx_results"].as_array().unwrap() { assert_eq!(result["code"], 0, "{result}"); }
        self.request("commit", json!({}));
        let info = self.info();
        assert_eq!(info.height, height);
        assert_eq!(finalized["app_hash"], info.app_hash);
        finalized
    }
    fn query(&mut self, path: &str) -> Value {
        let response = self.request("query", json!({"path":path,"height":0,"prove":false}));
        assert_eq!(response["code"], 0);
        serde_json::from_slice(&B64.decode(response["value"].as_str().unwrap()).unwrap()).unwrap()
    }
    fn assert_startup_rejected(mut self, expected_error: &str) {
        self.requests.take();
        let deadline = Instant::now() + FRAME_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let stderr = std::fs::read_to_string(&self.stderr).unwrap();
                assert!(!status.success(), "Invalid candidate started successfully: {stderr}");
                assert!(stderr.contains(expected_error), "Unexpected startup error: {stderr}");
                break;
            }
            assert!(Instant::now() < deadline, "Candidate startup rejection timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn close(mut self) {
        self.requests.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "Process exit: {status}; {}",
                    std::fs::read_to_string(&self.stderr).unwrap_or_default());
                break;
            }
            assert!(Instant::now() < deadline, "Process did not exit after pipe close");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for HandoverProcess {
    fn drop(&mut self) {
        self.requests.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

fn write_process_settings(root: &RootFixture) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = root._directory.path();
    let r = &root.root;
    let config = json!({"enabled":true,"profile":r.profile,"helper_path":r.helper_path,
        "helper_scratch_path":r.helper_scratch_path,"helper_sha256":r.helper_sha256,
        "max_helper_bytes":r.max_helper_bytes,"max_request_bytes":r.max_request_bytes,
        "timeout_ms":r.timeout_ms,"policy_path":dir.join("policy.json"),
        "request_path":dir.join("request.json"),"engine_genesis_path":r.engine_genesis_path,
        "max_engine_genesis_bytes":r.max_engine_genesis_bytes,"engine_genesis_sha512":r.engine_genesis_sha512,
        "release_manifest_path":r.release_manifest_path,"max_release_manifest_bytes":r.max_release_manifest_bytes,
        "release_manifest_sha512":r.release_manifest_sha512});
    std::fs::write(dir.join("process-root.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::write(dir.join("process-verifier.json"), serde_json::to_vec(&root.verifier).unwrap()).unwrap();
    ["consensus.json", "genesis.json", "policy.json", "request.json", "engine-genesis.json",
        "release-manifest.json", "process-root.json", "process-verifier.json"].iter()
        .map(|name| { let path = dir.join(name); let bytes = std::fs::read(&path).unwrap(); (path, bytes) }).collect()
}

fn executable_manifest(f: &Fixture, executable: &Path) -> Vec<u8> {
    use crate::runtime_candidate::{Executable, ManifestV1, Target};
    use sha2::Sha512;
    let bytes = std::fs::read(executable).unwrap();
    serde_json::to_vec(&ManifestV1 {
        schema: 1,
        chain_id: CHAIN.into(),
        app_genesis_sha256: f.config.app_state_sha256.clone(),
        target: Target { os: std::env::consts::OS.into(), arch: std::env::consts::ARCH.into() },
        consensus_stdio: Executable {
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            sha512: hex::encode(Sha512::digest(&bytes)),
        },
        migration_registry_sha256: upgrade::registry_sha256(),
    }).unwrap()
}

fn write_candidate_settings(root: &RootFixture, executable: &Path, manifest_path: &Path, label: &str) -> PathBuf {
    let path = root._directory.path().join(format!("process-candidate-{label}.json"));
    // macOS TempDir paths can start with /var, which aliases /private/var.
    // The production verifier deliberately refuses aliases for manifest inputs.
    let manifest = std::fs::canonicalize(manifest_path).unwrap();
    let settings = json!({"manifest_path":manifest,"max_manifest_bytes":65_536,
        "max_executable_bytes":std::fs::metadata(executable).unwrap().len().checked_add(1024 * 1024).unwrap()});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    path
}

fn process_block(height: u64, txs: &[Vec<u8>]) -> Value {
    json!({"height":height,"time_seconds":height as i64 * 10,"time_nanos":0,
        "hash":format!("{height:064x}"),"txs":txs.iter().map(|tx| B64.encode(tx)).collect::<Vec<_>>(),
        "misbehavior":[]})
}
fn configure_handover(f: &mut Fixture, path: &Path) {
    f.config.max_tx_bytes = 262_144;
    let genesis = f.config.app_state_sha256.clone();
    let policy = f.config.emergency.as_mut().unwrap();
    policy.schema = 2;
    policy.max_control_bytes = 262_144;
    policy.max_signatures = 3;
    policy.freeze_authority = authority(path, 11, 5, 3, "freeze");
    policy.resume_authority = authority(path, 21, 5, 3, "resume");
    policy.v2 = Some(emergency::PolicyV2 { genesis_sha256:genesis.clone(), authority_epoch:1,
        max_validity_blocks:4, max_anchor_age_blocks:4 });
    let initial_release = policy.release_sha512.clone();
    f.config.upgrade = Some(upgrade::Policy {
        schema:1, development_only:true, chain_id:CHAIN.into(), genesis_sha256:genesis.clone(),
        source_release_sha512:initial_release.clone(), authority_epoch:1,
        authority:authority(path,31,1,1,"upgrade"), initial_sequence:1,
        max_control_bytes:262_144, max_signatures:1,
        migration_bounds:upgrade::MigrationBounds { max_receipts:16,max_receipt_bytes:4*1024*1024,max_write_bytes:64*1024 },
    });
    f.config.release_handover = Some(handover::Policy {
        schema:1, development_only:true, chain_id:CHAIN.into(), genesis_sha256:genesis,
        initial_release_sha512:initial_release, initial_schema:0, authority_epoch:1,
        authority:authority(path,41,1,1,"handover"), initial_sequence:1,
        max_control_bytes:262_144,max_signatures:1,
    });
}
fn signature(root: &RootFixture, artifact: &[u8], action: &str, sequence: u64,
    height: u64, key: u8, purpose: &str) -> emergency::ControlSignature {
    let signed = sign(root._directory.path(),artifact,action,sequence,height,height,key);
    emergency::ControlSignature { key_id:format!("{purpose}-{key}"),
        signature_hex:hex::encode(B64.decode(signed["Request"]["Signature"].as_str().unwrap()).unwrap()) }
}
fn emergency_from_process(root: &RootFixture,f: &Fixture,app: &mut HandoverProcess,
    action: emergency::Action, release: &str, freeze_receipt: Option<&str>) -> Vec<u8> {
    let (head,status) = app.snapshot();
    let height = head.height + 1;
    let policy = f.config.emergency.as_ref().unwrap();
    let payload = emergency::Payload {
        schema:2,chain_id:CHAIN.into(),release_sha512:release.into(),action,
        sequence:status["emergency_control"]["next_sequence"].as_u64().unwrap(),
        parent_height:head.height,parent_app_hash:head.app_hash.clone(),target_height:height,
        v2:Some(emergency::PayloadV2 { genesis_sha256:f.config.app_state_sha256.clone(), authority_epoch:1,
            policy_sha256:policy.sha256().unwrap(),not_before_height:height,not_after_height:height,
            incident_sha256:"33".repeat(32),resume:if action==emergency::Action::Resume {
                Some(emergency::ResumeBinding { freeze_receipt_sha256:freeze_receipt.unwrap().into(),
                    restored_state_sha256:head.app_hash,readiness_evidence_sha256:"44".repeat(32) })
            } else { None } }),
    };
    let artifact = emergency::artifact_bytes(&payload).unwrap();
    let (first,purpose) = if action==emergency::Action::Freeze {(11,"freeze")} else {(21,"resume")};
    let signatures = (first..first+3).map(|key| signature(root,&artifact,"emergency",payload.sequence,height,key,purpose)).collect();
    serde_json::to_vec(&emergency::Control { kind:emergency::CONTROL_KIND_V2.into(),payload,signatures }).unwrap()
}
fn assert_only_emergency_release_differs(rejected: &[u8], accepted: &[u8], old_release: &str, active_release: &str) {
    let mut rejected: emergency::Control=serde_json::from_slice(rejected).unwrap();
    let accepted: emergency::Control=serde_json::from_slice(accepted).unwrap();
    assert_eq!(rejected.kind,accepted.kind);
    assert_eq!(rejected.payload.release_sha512,old_release);
    assert_eq!(accepted.payload.release_sha512,active_release);
    assert_ne!(old_release,active_release);
    assert_eq!(rejected.signatures.len(),3);
    assert_eq!(accepted.signatures.len(),3);
    for (old,new) in rejected.signatures.iter().zip(&accepted.signatures) {
        assert_eq!(old.key_id,new.key_id);
        assert_eq!(old.signature_hex.len(),29_792*2);
        assert_eq!(new.signature_hex.len(),29_792*2);
        assert_ne!(old.signature_hex,new.signature_hex,"Both payloads must receive their own signatures");
    }
    // Each payload was signed independently by the real disposable signer.
    // All chain, policy, sequence, anchor, incident and timing fields must match.
    rejected.payload.release_sha512=accepted.payload.release_sha512.clone();
    assert_eq!(rejected.payload,accepted.payload,"Negative fixture changed more than the active release binding");
}
fn upgrade_from_process(root: &RootFixture,f: &Fixture,app: &mut HandoverProcess, action: upgrade::Action) -> Vec<u8> {
    let (head,status)=app.snapshot();
    let payload=upgrade::Payload { schema:1,chain_id:CHAIN.into(),genesis_sha256:f.config.app_state_sha256.clone(),
        source_release_sha512:f.config.upgrade.as_ref().unwrap().source_release_sha512.clone(),authority_epoch:1,
        sequence:status["upgrade"]["next_sequence"].as_u64().unwrap(),parent_height:head.height,
        parent_app_hash:head.app_hash,target_height:head.height+1,action };
    let signed=signature(root,&upgrade::artifact_bytes(&payload).unwrap(),"upgrade",payload.sequence,payload.target_height,31,"upgrade");
    serde_json::to_vec(&upgrade::Control { kind:upgrade::CONTROL_KIND.into(),payload,signatures:vec![signed] }).unwrap()
}
fn handover_from_process(root: &RootFixture,f: &Fixture,app: &mut HandoverProcess,action: handover::Action) -> Vec<u8> {
    let (head,status)=app.snapshot();
    let policy=f.config.release_handover.as_ref().unwrap();
    let payload=handover::Payload { schema:1,chain_id:CHAIN.into(),genesis_sha256:f.config.app_state_sha256.clone(),
        policy_sha256:policy.sha256().unwrap(),source_release_sha512:status["release_handover"]["active_release_sha512"].as_str().unwrap().into(),
        authority_epoch:1,sequence:status["release_handover"]["next_sequence"].as_u64().unwrap(),
        parent_height:head.height,parent_app_hash:head.app_hash,target_height:head.height+1,action };
    let signed=signature(root,&handover::artifact_bytes(&payload).unwrap(),"upgrade",payload.sequence,payload.target_height,41,"handover");
    serde_json::to_vec(&handover::Control {kind:handover::CONTROL_KIND.into(),payload,signatures:vec![signed]}).unwrap()
}
fn last_emergency_receipt(app:&mut HandoverProcess)->String {
    app.query("/status")["emergency_control"]["last_receipt_sha256"].as_str().unwrap().into()
}
fn check_receipt(app:&mut HandoverProcess,digest:&str,sequence:u64,control:&[u8])->Value {
    let response=app.query(&format!("/emergency/receipt/{digest}"));
    assert_eq!(response["status"],"committed_receipt_reported");
    assert_eq!(response["sequence"],sequence);
    let receipt:emergency::Receipt=serde_json::from_value(response["receipt"].clone()).unwrap();
    assert_eq!(receipt.sha256().unwrap(),digest);
    assert_eq!(serde_json::to_vec(&receipt.control).unwrap(),control);
    let head=app.info();
    assert_eq!(response["context"]["height"],head.height);
    assert_eq!(response["context"]["app_hash"],head.app_hash);
    response["receipt"].clone()
}

#[test]
#[ignore="Requires two pinned handover executables, real SLH helper and disposable root/control signers"]
fn actual_signed_handover_pairs_migration_and_changes_executing_candidate() {
    let source=std::fs::canonicalize(std::env::var("DYT_HANDOVER_SOURCE_APP").expect("Explicit source app")).unwrap();
    let target=std::fs::canonicalize(std::env::var("DYT_HANDOVER_TARGET_APP").expect("Explicit target app")).unwrap();
    let source_hash=hex::encode(Sha256::digest(std::fs::read(&source).unwrap()));
    let target_hash=hex::encode(Sha256::digest(std::fs::read(&target).unwrap()));
    assert_ne!(source_hash,target_hash,"Handover requires distinct actual executable bytes");
    let mut f=Fixture::new();
    let source_manifest=executable_manifest(&f,&source);
    let target_manifest=executable_manifest(&f,&target);
    let source_release=hex::encode(Sha512::digest(&source_manifest));
    let target_release=hex::encode(Sha512::digest(&target_manifest));
    assert_ne!(source_release,target_release);
    let root=RootFixture::with_release_manifest(&mut f,&source_manifest,configure_handover);
    let mut immutable_inputs=write_process_settings(&root);
    let target_manifest_path=root._directory.path().join("target-release-manifest.json");
    std::fs::write(&target_manifest_path,&target_manifest).unwrap();
    let source_settings=write_candidate_settings(&root,&source,&root.root.release_manifest_path,"source");
    let target_settings=write_candidate_settings(&root,&target,&target_manifest_path,"target");
    for path in [&source_settings,&target_settings,&target_manifest_path] {
        immutable_inputs.push((path.to_path_buf(),std::fs::read(path).unwrap()));
    }
    let database=tempfile::tempdir().unwrap();
    let db=database.path().join("shared-handover-db");
    let mut a=HandoverProcess::open_with_candidate(&source,&root,&db,"handover-source-genesis",Some(&source_settings));
    let mut init=json!({"chain_id":CHAIN,"initial_height":1,"app_state_bytes":B64.encode(&f.genesis),"validators":f.config.validators});
    if let Some(lifecycle)=&f.config.lifecycle {
        init["evidence_max_age_blocks"]=json!(lifecycle.evidence_max_age_blocks);
        init["evidence_max_age_seconds"]=json!(lifecycle.evidence_max_age_seconds);
        init["evidence_max_age_nanos"]=json!(0);
    }
    a.request("init_chain",init);
    assert_eq!(a.info().height,0);
    assert_eq!(a.query("/status")["release_handover"]["active_release_sha512"],source_release);

    let freeze=emergency_from_process(&root,&f,&mut a,emergency::Action::Freeze,&source_release,None);
    a.commit_txs(1,&[freeze.clone()]);
    let freeze_receipt=last_emergency_receipt(&mut a);
    let resume=emergency_from_process(&root,&f,&mut a,emergency::Action::Resume,&source_release,Some(&freeze_receipt));
    a.commit_txs(2,&[resume.clone()]);
    let resume_receipt=last_emergency_receipt(&mut a);
    assert_eq!(a.query(&format!("/emergency/receipt/{freeze_receipt}"))["status"],"index_unavailable");
    let migration_plan=plan(&f);
    let migration_admit=upgrade_from_process(&root,&f,&mut a,upgrade::Action::Admit {plan:migration_plan.clone()});
    a.commit_txs(3,&[migration_admit]);
    let release_plan=handover::ReleasePlan {target_release_sha512:target_release.clone(),
        transition:handover::Transition::ReceiptIndexV1 {migration_sha256:upgrade::migration_sha256()},
        authorization_sha256:"77".repeat(32)};
    let release_admit=handover_from_process(&root,&f,&mut a,handover::Action::Admit {plan:release_plan.clone()});
    a.commit_txs(4,&[release_admit]);
    let admitted=a.snapshot();
    assert_eq!(admitted.1["release_handover"]["next_sequence"],2);
    assert_eq!(admitted.1["upgrade"]["next_sequence"],2);
    a.close();

    HandoverProcess::open_with_candidate(&target,&root,&db,"handover-target-premature",Some(&target_settings))
        .assert_startup_rejected("Candidate manifest digest mismatch");
    let mut a=HandoverProcess::open_with_candidate(&source,&root,&db,"handover-source-admitted",Some(&source_settings));
    assert_eq!(a.snapshot(),admitted,"Premature target startup changed committed state");
    let migration_activate=upgrade_from_process(&root,&f,&mut a,upgrade::Action::Activate {
        plan:migration_plan,admission_receipt_sha256:admitted.1["upgrade"]["pending"]["admission_receipt_sha256"].as_str().unwrap().into(),
        emergency_receipt_sha256:Some(resume_receipt.clone()),evidence_sha256:"66".repeat(32)});
    let mut action=handover::Action::Activate {plan:release_plan,
        admission_receipt_sha256:admitted.1["release_handover"]["pending"]["admission_receipt_sha256"].as_str().unwrap().into(),
        emergency_receipt_sha256:Some(resume_receipt.clone()),evidence_sha256:"88".repeat(32),
        upgrade_activation_sha256:Some("00".repeat(32))};
    // Both signatures are valid. Only the signed pairing digest is wrong.
    let wrong_pair=handover_from_process(&root,&f,&mut a,action.clone());
    let wrong_block=process_block(5,&[migration_activate.clone(),wrong_pair]);
    assert_eq!(a.request("process_proposal",wrong_block.clone())["accept"],false);
    a.rejected("finalize_block",wrong_block,"Paired upgrade control hash differs");
    a.request("commit",json!({}));
    assert_eq!(a.snapshot(),admitted,"Rejected signed pair changed state/head/sequences");
    assert_eq!(a.query(&format!("/emergency/receipt/{freeze_receipt}"))["status"],"index_unavailable");
    if let handover::Action::Activate {upgrade_activation_sha256,..}=&mut action {
        *upgrade_activation_sha256=Some(hex::encode(Sha256::digest(&migration_activate)));
    }
    let correct_pair=handover_from_process(&root,&f,&mut a,action);
    let activation_block=process_block(5,&[migration_activate.clone(),correct_pair.clone()]);
    let activation_result=a.commit_txs(5,&[migration_activate,correct_pair]);
    // A committed activation can be replayed for a lost acknowledgement.
    assert_eq!(a.request("finalize_block",activation_block),activation_result);
    a.request("commit",json!({}));
    let activated=a.snapshot();
    assert_eq!(activated.1["release_handover"]["active_release_sha512"],target_release);
    assert_eq!(activated.1["release_handover"]["active_schema"],1);
    assert_eq!(activated.1["release_handover"]["next_sequence"],3);
    assert!(activated.1["release_handover"]["pending"].is_null());
    assert_eq!(activated.1["upgrade"]["active_schema"],1);
    assert_eq!(activated.1["upgrade"]["next_sequence"],3);
    assert_eq!(activated.1["emergency_control"]["blocks_upgrade"],true);
    let first_record=check_receipt(&mut a,&freeze_receipt,1,&freeze);
    let second_record=check_receipt(&mut a,&resume_receipt,2,&resume);
    let old_source_new_control=emergency_from_process(&root,&f,&mut a,emergency::Action::Freeze,&source_release,None);
    a.rejected("check_tx",json!({"type":"new","tx":B64.encode(&old_source_new_control)}),"different runtime executable");
    a.rejected("finalize_block",process_block(6,&[]),"different runtime executable");
    let proposal=a.raw("process_proposal",process_block(6,&[]));
    assert!(proposal["ok"]==false || proposal["result"]["accept"]==false,"Old source accepted later proposal: {proposal}");
    assert_eq!(a.snapshot(),activated,"Source crossed committed execution barrier");
    a.close();
    HandoverProcess::open_with_candidate(&source,&root,&db,"handover-source-obsolete",Some(&source_settings))
        .assert_startup_rejected("Candidate manifest digest mismatch");

    let mut b=HandoverProcess::open_with_candidate(&target,&root,&db,"handover-target-active",Some(&target_settings));
    assert_eq!(b.snapshot(),activated,"Target did not reopen the exact activated state");
    assert_eq!(check_receipt(&mut b,&freeze_receipt,1,&freeze),first_record);
    assert_eq!(check_receipt(&mut b,&resume_receipt,2,&resume),second_record);
    let refused=b.request("check_tx",json!({"type":"new","tx":B64.encode(&old_source_new_control)}));
    assert_ne!(refused["code"],0,"Target accepted a fresh old-release control");
    assert_eq!(refused["log"],"Invalid transaction or observation: Emergency payload policy binding","{refused}");
    assert_eq!(b.snapshot(),activated);
    let target_freeze=emergency_from_process(&root,&f,&mut b,emergency::Action::Freeze,&target_release,None);
    assert_only_emergency_release_differs(&old_source_new_control,&target_freeze,&source_release,&target_release);
    b.commit_txs(6,&[target_freeze.clone()]);
    let target_freeze_receipt=last_emergency_receipt(&mut b);
    check_receipt(&mut b,&target_freeze_receipt,3,&target_freeze);
    let target_resume=emergency_from_process(&root,&f,&mut b,emergency::Action::Resume,&target_release,Some(&target_freeze_receipt));
    b.commit_txs(7,&[target_resume.clone()]);
    let target_resume_receipt=last_emergency_receipt(&mut b);
    let final_state=b.snapshot();
    assert_eq!(final_state.1["emergency_control"]["next_sequence"],5);
    assert_eq!(final_state.1["emergency_control"]["frozen"],false);
    assert_eq!(final_state.1["emergency_control"]["blocks_upgrade"],true);
    assert_eq!(final_state.1["release_handover"]["active_release_sha512"],target_release);
    b.close();
    let mut b=HandoverProcess::open_with_candidate(&target,&root,&db,"handover-target-later-replay",Some(&target_settings));
    assert_eq!(b.snapshot(),final_state);
    for (digest,sequence,control) in [(&freeze_receipt,1,&freeze),(&resume_receipt,2,&resume),
        (&target_freeze_receipt,3,&target_freeze),(&target_resume_receipt,4,&target_resume)] {
        check_receipt(&mut b,digest,sequence,control);
    }
    // Returning to A is a new signed forward transition. The migrated schema
    // remains 1, and no original admission or migration activation is reused.
    let return_plan=handover::ReleasePlan {target_release_sha512:source_release.clone(),
        transition:handover::Transition::SchemaPreserving {schema:1},authorization_sha256:"99".repeat(32)};
    let return_admit=handover_from_process(&root,&f,&mut b,handover::Action::Admit {plan:return_plan.clone()});
    b.commit_txs(8,&[return_admit]);
    let return_admitted=b.snapshot();
    assert_eq!(return_admitted.1["release_handover"]["next_sequence"],4);
    assert_eq!(return_admitted.1["release_handover"]["active_release_sha512"],target_release);
    assert_eq!(return_admitted.1["upgrade"],final_state.1["upgrade"]);
    b.close();
    HandoverProcess::open_with_candidate(&source,&root,&db,"handover-return-premature",Some(&source_settings))
        .assert_startup_rejected("Candidate manifest digest mismatch");
    let mut b=HandoverProcess::open_with_candidate(&target,&root,&db,"handover-return-admitted",Some(&target_settings));
    assert_eq!(b.snapshot(),return_admitted,"Early return candidate changed committed state");
    let return_activate=handover_from_process(&root,&f,&mut b,handover::Action::Activate {
        plan:return_plan,
        admission_receipt_sha256:return_admitted.1["release_handover"]["pending"]["admission_receipt_sha256"].as_str().unwrap().into(),
        emergency_receipt_sha256:Some(target_resume_receipt.clone()),evidence_sha256:"aa".repeat(32),
        upgrade_activation_sha256:None});
    b.commit_txs(9,&[return_activate]);
    let returned=b.snapshot();
    assert_eq!(returned.1["release_handover"]["active_release_sha512"],source_release);
    assert_eq!(returned.1["release_handover"]["active_schema"],1);
    assert_eq!(returned.1["release_handover"]["next_sequence"],5);
    assert!(returned.1["release_handover"]["pending"].is_null());
    assert_eq!(returned.1["upgrade"],final_state.1["upgrade"],"Schema-preserving return reran migration");
    assert_eq!(returned.1["emergency_control"],final_state.1["emergency_control"]);
    let obsolete_target_control=emergency_from_process(&root,&f,&mut b,emergency::Action::Freeze,&target_release,None);
    b.rejected("check_tx",json!({"type":"new","tx":B64.encode(&obsolete_target_control)}),"different runtime executable");
    b.rejected("finalize_block",process_block(10,&[]),"different runtime executable");
    assert_eq!(b.snapshot(),returned);
    b.close();
    HandoverProcess::open_with_candidate(&target,&root,&db,"handover-target-obsolete-return",Some(&target_settings))
        .assert_startup_rejected("Candidate manifest digest mismatch");
    let mut a=HandoverProcess::open_with_candidate(&source,&root,&db,"handover-source-returned",Some(&source_settings));
    assert_eq!(a.snapshot(),returned,"Returned source did not reopen exact schema-1 state");
    for (digest,sequence,control) in [(&freeze_receipt,1,&freeze),(&resume_receipt,2,&resume),
        (&target_freeze_receipt,3,&target_freeze),(&target_resume_receipt,4,&target_resume)] {
        check_receipt(&mut a,digest,sequence,control);
    }
    let refused=a.request("check_tx",json!({"type":"new","tx":B64.encode(&obsolete_target_control)}));
    assert_ne!(refused["code"],0,"Returned source accepted fresh obsolete-target control");
    assert_eq!(refused["log"],"Invalid transaction or observation: Emergency payload policy binding","{refused}");
    assert_eq!(a.snapshot(),returned);
    let returned_freeze=emergency_from_process(&root,&f,&mut a,emergency::Action::Freeze,&source_release,None);
    assert_only_emergency_release_differs(&obsolete_target_control,&returned_freeze,&target_release,&source_release);
    a.commit_txs(10,&[returned_freeze.clone()]);
    let returned_freeze_receipt=last_emergency_receipt(&mut a);
    let returned_resume=emergency_from_process(&root,&f,&mut a,emergency::Action::Resume,&source_release,Some(&returned_freeze_receipt));
    a.commit_txs(11,&[returned_resume.clone()]);
    let returned_resume_receipt=last_emergency_receipt(&mut a);
    let complete=a.snapshot();
    assert_eq!(complete.1["emergency_control"]["next_sequence"],7);
    assert_eq!(complete.1["emergency_control"]["frozen"],false);
    assert_eq!(complete.1["emergency_control"]["blocks_upgrade"],true);
    assert_eq!(complete.1["release_handover"],returned.1["release_handover"]);
    assert_eq!(complete.1["upgrade"],final_state.1["upgrade"]);
    a.close();
    let mut a=HandoverProcess::open_with_candidate(&source,&root,&db,"handover-return-final-replay",Some(&source_settings));
    assert_eq!(a.snapshot(),complete);
    for (digest,sequence,control) in [(&freeze_receipt,1,&freeze),(&resume_receipt,2,&resume),
        (&target_freeze_receipt,3,&target_freeze),(&target_resume_receipt,4,&target_resume),
        (&returned_freeze_receipt,5,&returned_freeze),(&returned_resume_receipt,6,&returned_resume)] {
        check_receipt(&mut a,digest,sequence,control);
    }
    a.close();
    for (path,bytes) in immutable_inputs {assert_eq!(std::fs::read(&path).unwrap(),bytes,"Changed immutable input: {}",path.display());}
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&source).unwrap())),source_hash);
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&target).unwrap())),target_hash);
    println!("PASS signed handover: source_sha256={source_hash} target_sha256={target_hash}; 11 committed blocks; 6 historic/later emergency receipts; paired migration; signed schema-preserving forward return; both execution barriers; production disabled");
}
