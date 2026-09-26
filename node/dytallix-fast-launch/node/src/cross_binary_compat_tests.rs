//! Actual A/B process compatibility. This does not authorize a release handover.
use super::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FRAME_LIMIT: u64 = 8 * 1024 * 1024;
const FRAME_TIMEOUT: Duration = Duration::from_secs(120);

struct PipeApp {
    child: Child,
    requests: Option<mpsc::Sender<Value>>,
    responses: mpsc::Receiver<std::result::Result<Value, String>>,
    worker: Option<JoinHandle<()>>,
    stderr: PathBuf,
}
impl PipeApp {
    fn open(binary: &Path, root: &RootFixture, database: &Path, label: &str) -> Self {
        Self::open_with_candidate(binary, root, database, label, None)
    }
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
    fn request(&mut self, method: &str, payload: Value) -> Value {
        self.requests.as_ref().unwrap().send(json!({"method":method,"payload":payload})).unwrap();
        let response = match self.responses.recv_timeout(FRAME_TIMEOUT) {
            Ok(Ok(value)) => value,
            error => panic!("Process request {method} failed: {error:?}; stderr: {}",
                std::fs::read_to_string(&self.stderr).unwrap_or_default()),
        };
        assert_eq!(response["ok"], true, "{method}: {response}");
        response["result"].clone()
    }
    fn query(&mut self, path: &str) -> Value {
        let response = self.request("query", json!({"path":path,"height":0,"prove":false}));
        assert_eq!(response["code"], 0);
        serde_json::from_slice(&B64.decode(response["value"].as_str().unwrap()).unwrap()).unwrap()
    }
    fn assert_state(&mut self, model: &ConsensusApplication) {
        let actual = self.request("info", json!({}));
        let expected = model.info().unwrap();
        assert_eq!(actual["height"], expected.height);
        assert_eq!(actual["app_hash"], expected.app_hash);
        assert_eq!(self.query("/status"), model.query().unwrap());
    }
    fn assert_receipt(&mut self, model: &ConsensusApplication, digest: &str) {
        assert_eq!(self.query(&format!("/emergency/receipt/{digest}")),
            model.query_emergency_receipt(digest).unwrap());
    }
    fn commit(&mut self, model: &mut ConsensusApplication, height: u64, tx: Vec<u8>) {
        let check = self.request("check_tx", json!({"type":"new","tx":B64.encode(&tx)}));
        assert_eq!(check["code"], 0, "{check}");
        let input = block(height, vec![tx.clone()]);
        let payload = json!({"height":input.height,"time_seconds":input.time_seconds,
            "time_nanos":input.time_nanos,"hash":input.hash,"txs":[B64.encode(&tx)],"misbehavior":[]});
        assert_eq!(self.request("process_proposal", payload.clone())["accept"], true);
        let actual = self.request("finalize_block", payload);
        let expected = commit(model, height, vec![tx]);
        assert_eq!(actual, serde_json::to_value(expected).unwrap());
        self.request("commit", json!({}));
        self.assert_state(model);
    }
    fn assert_startup_rejected(mut self, database: &Path, expected_error: &str) {
        self.requests.take();
        let deadline = Instant::now() + FRAME_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let stderr = std::fs::read_to_string(&self.stderr).unwrap();
                assert!(!status.success(), "Invalid candidate started successfully: {stderr}");
                assert!(stderr.contains(expected_error), "Unexpected startup error: {stderr}");
                assert!(!database.exists(), "Rejected startup created database files");
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
impl Drop for PipeApp {
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

fn last_emergency(model: &ConsensusApplication) -> String {
    emergency_state(&model.storage, &model.config).unwrap().unwrap()
        .last_receipt_sha256().unwrap().to_owned()
}

#[test]
#[ignore = "Requires distinct pinned baseline/candidate apps, real SLH helper and disposable fixture signers"]
fn actual_distinct_processes_preserve_v1_history_and_index_across_replacement() {
    let baseline = PathBuf::from(std::env::var("DYT_BASELINE_APP").expect("Explicit baseline app"));
    let candidate = PathBuf::from(std::env::var("DYT_CANDIDATE_APP").expect("Explicit candidate app"));
    assert!(baseline.is_absolute() && candidate.is_absolute());
    let baseline_hash = hex::encode(Sha256::digest(std::fs::read(&baseline).unwrap()));
    let candidate_hash = hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap()));
    assert_ne!(baseline_hash, candidate_hash, "This test requires distinct executable bytes");
    println!("Process compatibility baseline_sha256={baseline_hash} candidate_sha256={candidate_hash}");
    let mut f = Fixture::new();
    let root = root(&mut f, true);
    let preserved_inputs = write_process_settings(&root);
    let model_db = tempfile::tempdir().unwrap();
    let actual_db = tempfile::tempdir().unwrap();
    let mut model = root.initialized(&f, model_db.path());
    let mut a = PipeApp::open(&baseline, &root, actual_db.path(), "a-genesis");
    let initial = a.request("init_chain", json!({"chain_id":CHAIN,"initial_height":1,
        "app_state_bytes":B64.encode(&f.genesis),"validators":f.config.validators,
        "evidence_max_age_blocks":f.config.lifecycle.as_ref().unwrap().evidence_max_age_blocks,
        "evidence_max_age_seconds":f.config.lifecycle.as_ref().unwrap().evidence_max_age_seconds,
        "evidence_max_age_nanos":0}));
    assert_eq!(initial["app_hash"], model.info().unwrap().app_hash);
    a.assert_state(&model);
    let freeze = emergency_control(&root, &f, &model, emergency::Action::Freeze, &model.info().unwrap(), 1, 2);
    a.commit(&mut model, 1, freeze);
    let first_receipt = last_emergency(&model);
    assert_eq!(a.query(&format!("/emergency/receipt/{first_receipt}"))["status"], "index_unavailable");
    let resume = emergency_control(&root, &f, &model, emergency::Action::Resume, &model.info().unwrap(), 2, 3);
    a.commit(&mut model, 2, resume);
    let second_receipt = last_emergency(&model);
    let admit = upgrade_control(&root, &f, &model, upgrade::Action::Admit { plan: plan(&f) });
    a.commit(&mut model, 3, admit);
    let before = a.query("/status");
    assert_eq!(before["upgrade"]["next_sequence"], 2);
    assert_eq!(before["upgrade"]["active_schema"], 0);
    assert!(!before["upgrade"]["pending"].is_null());
    a.close();

    let mut b = PipeApp::open(&candidate, &root, actual_db.path(), "b-before-migration");
    b.assert_state(&model);
    assert_eq!(b.query("/status"), before);
    b.assert_receipt(&model, &first_receipt);
    let activate = activation(&root, &f, &model);
    b.commit(&mut model, 4, activate);
    let migrated = b.query("/status");
    assert_eq!(migrated["upgrade"]["active_schema"], 1);
    assert_eq!(migrated["upgrade"]["next_sequence"], 3);
    assert!(migrated["upgrade"]["pending"].is_null());
    assert_eq!(migrated["emergency_control"]["blocks_upgrade"], true);
    for digest in [&first_receipt, &second_receipt] {
        b.assert_receipt(&model, digest);
        assert_eq!(b.query(&format!("/emergency/receipt/{digest}"))["status"], "committed_receipt_reported");
    }
    b.close();
    let mut b = PipeApp::open(&candidate, &root, actual_db.path(), "b-after-migration");
    b.assert_state(&model);
    assert_eq!(b.query("/status"), migrated);
    b.assert_receipt(&model, &first_receipt);
    b.close();

    // A already implements this exact schema and migration. This is a compatible
    // reopening test, not permission to roll back to an arbitrary older release.
    let mut a = PipeApp::open(&baseline, &root, actual_db.path(), "a-after-migration");
    a.assert_state(&model);
    a.assert_receipt(&model, &first_receipt);
    let freeze = emergency_control(&root, &f, &model, emergency::Action::Freeze, &model.info().unwrap(), 5, 6);
    a.commit(&mut model, 5, freeze);
    let third_receipt = last_emergency(&model);
    a.assert_receipt(&model, &third_receipt);
    a.close();

    let mut b = PipeApp::open(&candidate, &root, actual_db.path(), "b-later-history");
    b.assert_state(&model);
    b.assert_receipt(&model, &third_receipt);
    let resume = emergency_control(&root, &f, &model, emergency::Action::Resume, &model.info().unwrap(), 6, 7);
    b.commit(&mut model, 6, resume);
    let fourth_receipt = last_emergency(&model);
    let final_status = b.query("/status");
    assert_eq!(final_status["emergency_control"]["next_sequence"], 5);
    assert_eq!(final_status["emergency_control"]["frozen"], false);
    assert_eq!(final_status["upgrade"]["next_sequence"], 3);
    b.close();

    let mut a = PipeApp::open(&baseline, &root, actual_db.path(), "a-final");
    a.assert_state(&model);
    assert_eq!(a.query("/status"), final_status);
    for digest in [&first_receipt, &second_receipt, &third_receipt, &fourth_receipt] {
        a.assert_receipt(&model, digest);
    }
    a.close();
    for (path, bytes) in preserved_inputs {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "Input changed: {}", path.display());
    }
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&baseline).unwrap())), baseline_hash);
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap())), candidate_hash);
    println!("PASS: six committed blocks, four emergency receipts, admission and activation, six A/B process lifetimes; production disabled; no executable-bound handover claim");
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

fn write_candidate_settings(root: &RootFixture, executable: &Path) -> PathBuf {
    let path = root._directory.path().join("process-candidate.json");
    // macOS TempDir paths can start with /var, which aliases /private/var.
    // The production verifier deliberately refuses aliases for manifest inputs.
    let manifest = std::fs::canonicalize(&root.root.release_manifest_path).unwrap();
    let settings = json!({"manifest_path":manifest,"max_manifest_bytes":65_536,
        "max_executable_bytes":std::fs::metadata(executable).unwrap().len().checked_add(1024 * 1024).unwrap()});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    path
}

#[test]
#[ignore = "Requires pinned candidate and baseline apps plus real SLH root helper and disposable fixture signers"]
fn actual_candidate_startup_binds_signed_manifest_to_running_executable() {
    let candidate = std::fs::canonicalize(std::env::var("DYT_CANDIDATE_APP").expect("Explicit candidate app")).unwrap();
    let baseline = std::fs::canonicalize(std::env::var("DYT_BASELINE_APP").expect("Explicit baseline app")).unwrap();
    let candidate_hash = hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap()));
    let baseline_hash = hex::encode(Sha256::digest(std::fs::read(&baseline).unwrap()));
    assert_ne!(candidate_hash, baseline_hash);

    let mut f = Fixture::new();
    let manifest = executable_manifest(&f, &candidate);
    let root = RootFixture::with_release_manifest(&mut f, &manifest, |_, _| {});
    let preserved_inputs = write_process_settings(&root);
    let settings = write_candidate_settings(&root, &candidate);
    let settings_before = std::fs::read(&settings).unwrap();
    let database = tempfile::tempdir().unwrap();
    let actual_path = database.path().join("candidate-db");
    let model_path = database.path().join("model-db");
    let mut model = root.initialized(&f, &model_path);
    let mut process = PipeApp::open_with_candidate(&candidate, &root, &actual_path,
        "candidate-bound-initial", Some(&settings));
    let initialized = process.request("init_chain", json!({"chain_id":CHAIN,"initial_height":1,
        "app_state_bytes":B64.encode(&f.genesis),"validators":f.config.validators,
        "evidence_max_age_blocks":f.config.lifecycle.as_ref().unwrap().evidence_max_age_blocks,
        "evidence_max_age_seconds":f.config.lifecycle.as_ref().unwrap().evidence_max_age_seconds,
        "evidence_max_age_nanos":0}));
    assert_eq!(initialized["app_hash"], model.info().unwrap().app_hash);
    process.assert_state(&model);
    // Commit a real empty block, so restart must reopen durable post-genesis state.
    let input = block(1, vec![]);
    let finalized = process.request("finalize_block", json!({"height":input.height,
        "time_seconds":input.time_seconds,"time_nanos":input.time_nanos,"hash":input.hash,
        "txs":[],"misbehavior":[]}));
    assert_eq!(finalized, serde_json::to_value(commit(&mut model, 1, vec![])).unwrap());
    process.request("commit", json!({}));
    process.assert_state(&model);
    process.close();
    let mut restarted = PipeApp::open_with_candidate(&candidate, &root, &actual_path,
        "candidate-bound-restart", Some(&settings));
    restarted.assert_state(&model);
    restarted.close();
    for (path, bytes) in preserved_inputs {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "Root input changed: {}", path.display());
    }
    assert_eq!(std::fs::read(&settings).unwrap(), settings_before);

    // A fresh valid root signature authorizes A's manifest. Starting B with that
    // manifest must fail on the executable identity before opening any database.
    let mut mismatch_fixture = Fixture::new();
    let mismatch_manifest = executable_manifest(&mismatch_fixture, &baseline);
    let mismatch_root = RootFixture::with_release_manifest(&mut mismatch_fixture, &mismatch_manifest, |_, _| {});
    let mismatch_inputs = write_process_settings(&mismatch_root);
    let mismatch_settings = write_candidate_settings(&mismatch_root, &candidate);
    let mismatch_db = database.path().join("wrong-executable-db");
    PipeApp::open_with_candidate(&candidate, &mismatch_root, &mismatch_db,
        "candidate-wrong-executable", Some(&mismatch_settings))
        .assert_startup_rejected(&mismatch_db, "Candidate executable");
    for (path, bytes) in mismatch_inputs { assert_eq!(std::fs::read(path).unwrap(), bytes); }

    // Alter canonical manifest bytes without changing the root-signed request.
    let mut altered: crate::runtime_candidate::ManifestV1 = serde_json::from_slice(&manifest).unwrap();
    altered.chain_id = "different-disposable-chain".into();
    std::fs::write(&root.root.release_manifest_path, serde_json::to_vec(&altered).unwrap()).unwrap();
    let changed_db = database.path().join("changed-manifest-db");
    PipeApp::open_with_candidate(&candidate, &root, &changed_db,
        "candidate-changed-manifest", Some(&settings))
        .assert_startup_rejected(&changed_db, "Candidate manifest digest mismatch");
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap())), candidate_hash);
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&baseline).unwrap())), baseline_hash);
    println!("PASS: actual candidate root-bound startup and restart; wrong executable and changed manifest rejected before database creation; no release handover or production acceptance claim");
}
