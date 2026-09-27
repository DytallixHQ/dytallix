//! State sync join qualification (state sync v1, step C5). Runs the real
//! guarded binaries as a four-validator loopback chain: nodes 0 to 2 commit
//! and write snapshots, and node 3 joins late by state sync from light blocks
//! exported off node 2. Linux only; a qualification fixture, not a service.
//!
//! This process is the owner parent of every child (the release-runtime
//! owner protocol). It installs neither no_new_privs nor a seccomp filter:
//! each child's guard requires both, so run it where they are already
//! applied, such as a container with the production deny list or a systemd
//! unit with the rendered `SystemCallFilter`.
//!
//! `state-sync-join --bin DIR --work NEW_DIR`, where DIR holds
//! `consensus_stdio`, `dytallix-comet-bridge`, `dytallix-pqc-engine`,
//! `dytallix-comet-fixture` and `dytallix-light-export`. It prints one JSON
//! summary and exits zero only if node 3 restored a snapshot, caught up and
//! follows the chain with the same application hash.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("state-sync-join runs only on Linux");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match linux::run() {
        Ok(summary) => {
            println!("{summary}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("state-sync-join failed: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{bail, ensure, Context, Result};
    use dytallix_release_runtime::ownership::{self, Bootstrap, OwnerThread, Role};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha512};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const CHAIN: &str = "c5-state-sync-join";
    const BASE_PORT: u16 = 28650;
    /// Synthetic E05 inputs for this fixture only.
    const SNAPSHOT_INTERVAL: u64 = 5;
    const SNAPSHOT_KEEP: u64 = 10;
    /// Node 3 trusts this height; the export runs from it to the chain head.
    const TRUST_HEIGHT: u64 = 10;
    const EXPORT_AFTER: u64 = 22;
    const TRUST_PERIOD: &str = "24h0m0s";
    const STEP_TIMEOUT: Duration = Duration::from_secs(120);
    const JOIN_TIMEOUT: Duration = Duration::from_secs(240);

    struct Paths {
        bin: PathBuf,
        work: PathBuf,
        net: PathBuf,
        logs: PathBuf,
        light: PathBuf,
    }
    impl Paths {
        fn home(&self, node: usize) -> PathBuf {
            self.net.join(format!("node{node}"))
        }
        fn binary(&self, name: &str) -> PathBuf {
            self.bin.join(name)
        }
    }

    fn arguments() -> Result<(PathBuf, PathBuf)> {
        let mut args = std::env::args().skip(1);
        let (mut bin, mut work) = (None, None);
        while let Some(flag) = args.next() {
            let value = args.next().context("Each flag takes a value")?;
            match flag.as_str() {
                "--bin" => bin = Some(PathBuf::from(value)),
                "--work" => work = Some(PathBuf::from(value)),
                _ => bail!("usage: state-sync-join --bin DIR --work NEW_DIR"),
            }
        }
        let (bin, work) = (bin.context("--bin is required")?, work.context("--work is required")?);
        ensure!(bin.is_absolute() && work.is_absolute(), "Paths must be absolute");
        Ok((bin, work))
    }

    /// Each child's guard requires both; this parent only checks them.
    fn check_confinement() -> Result<()> {
        let status = fs::read_to_string("/proc/self/status")?;
        let field = |name: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .map(str::trim)
                .unwrap_or("")
                .to_owned()
        };
        ensure!(
            field("NoNewPrivs:") == "1" && field("Seccomp:") == "2",
            "Run under no_new_privs and a seccomp filter (NoNewPrivs {}, Seccomp {})",
            field("NoNewPrivs:"),
            field("Seccomp:")
        );
        Ok(())
    }

    fn private_dir(path: &Path) -> Result<()> {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .with_context(|| format!("Create {}", path.display()))
    }

    fn private_file(path: &Path, data: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(data)?;
        Ok(file.sync_all()?)
    }

    fn log_file(paths: &Paths, name: &str) -> Result<File> {
        Ok(OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(paths.logs.join(name))?)
    }

    /// A compact native genesis for the fixture's four validators, each
    /// bonded by one funded owner.
    fn native_genesis() -> Result<Vec<u8>> {
        use dytallix_fast_node::crypto::{ActivePQC, PQC};
        let (_, public) = ActivePQC::keypair();
        let owner = dytallix_fast_node::addr::initial_address(
            dytallix_fast_node::addr::AddressNetwork::Development,
            CHAIN,
            dytallix_fast_node::addr::OriginKeyAlgorithm::MlDsa65,
            &public,
        )?;
        let validators: Vec<Value> = (0..4)
            .map(|i| json!({"address": format!("validator-{i}"), "active": true, "jailed": false}))
            .collect();
        let positions: Vec<Value> = (0..4)
            .map(|i| json!({"owner": owner, "validator": format!("validator-{i}"), "amount_udgt": "100"}))
            .collect();
        Ok(serde_json::to_vec(&json!({
            "chain_id": CHAIN,
            "accounts": [{"address": owner,
                "balances": {"udgt": "1000", "udrt": "1000000"},
                "vesting": {"kind": "unlocked"}}],
            "staking": {"delegations": [{"delegator": owner, "amount_udgt": "400"}]},
            "reward_v2": {"version": 2, "activation_height": 1, "decimals": 6,
                "profile": "development", "max_validators": 4, "max_positions": 8,
                "validators": validators, "positions": positions},
            "adaptive_issuance": {"version": 1, "profile": "development", "decimals": 6,
                "epoch_blocks": 2, "initial_epoch_budget_udrt": "1001", "max_recorded_epochs": 8,
                "controller": {"target_ppm": 500000, "shock_threshold_ppm": 100000,
                    "volatility_threshold_ppm": 1000000, "window_samples": 2,
                    "integral_min": -2000000, "integral_max": 2000000,
                    "soft": {"proportional": 0, "integral": 0, "derivative": 0},
                    "hard": {"proportional": 0, "integral": 0, "derivative": 0},
                    "base_udrt": 1001, "min_udrt": 1001, "max_udrt": 1001}}
        }))?)
    }

    fn run_tool(paths: &Paths, name: &str, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new(paths.binary(name))
            .args(args)
            .stdin(Stdio::null())
            .stderr(log_file(paths, &format!("{name}.log"))?)
            .output()
            .with_context(|| format!("Run {name}"))?;
        ensure!(output.status.success(), "{name} failed: {}", output.status);
        Ok(output.stdout)
    }

    fn pipe() -> Result<(OwnedFd, OwnedFd)> {
        let mut fds = [-1; 2];
        ensure!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0, "pipe2 failed");
        Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
    }

    /// A close-on-exec duplicate above the descriptors a child receives.
    fn high(fd: RawFd) -> Result<OwnedFd> {
        let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 16) };
        ensure!(copy >= 0, "fcntl duplicate failed");
        Ok(unsafe { OwnedFd::from_raw_fd(copy) })
    }

    fn context() -> ([u8; 64], String) {
        let digest: [u8; 64] = Sha512::digest(b"dytallix-state-sync-join-qualification").into();
        (digest, hex::encode(digest))
    }

    /// Spawn one owned child: nothing inherited but its standard streams,
    /// the ownership descriptors and, for the bridge, the application pipes
    /// at 3 and 4. Returns after the child's guard admitted it.
    fn spawn_owned(
        owner: &OwnerThread,
        role: Role,
        mut command: Command,
        channel: Option<(RawFd, RawFd)>,
    ) -> Result<Child> {
        unsafe {
            command.pre_exec(move || {
                // Mark everything from 3 close-on-exec; the ownership and pipe
                // descriptors are installed after this.
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some((input, output)) = channel {
                    if libc::dup2(input, 3) != 3 || libc::dup2(output, 4) != 4 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let (digest, _) = context();
        let deadline = ownership::monotonic_deadline(Duration::from_secs(30))?;
        let mut bootstrap = Bootstrap::prepare(owner, role, digest, deadline)?;
        let mut child = bootstrap.spawn(&mut command)?;
        drop(command);
        if let Err(error) = bootstrap.await_ready().and_then(|()| bootstrap.release()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.context(format!("Owned {role:?} not admitted")));
        }
        Ok(child)
    }

    fn wait_for(path: &Path, what: &str, children: &mut [Child]) -> Result<()> {
        let deadline = Instant::now() + STEP_TIMEOUT;
        while !path.exists() {
            for child in children.iter_mut() {
                if let Some(status) = child.try_wait()? {
                    bail!("A child exited before {what}: {status}");
                }
            }
            ensure!(Instant::now() < deadline, "Timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    struct Node {
        index: usize,
        /// Engine, bridge and application, stopped in that order.
        children: Vec<Child>,
    }

    struct Launch<'a> {
        archive: bool,
        snapshots: bool,
        light_blocks: Option<&'a Path>,
        /// Write both processes' metrics files (metrics v1) every second.
        metrics: bool,
    }

    fn start_node(owner: &OwnerThread, paths: &Paths, index: usize, launch: &Launch) -> Result<Node> {
        let home = paths.home(index);
        let (_, context_hex) = context();
        let snapshot_dir = home.join("snapshots");
        let log = |role: &str| log_file(paths, &format!("node{index}-{role}.log"));
        let (app_in_read, app_in_write) = pipe()?;
        let (app_out_read, app_out_write) = pipe()?;

        let mut app = Command::new(paths.binary("consensus_stdio"));
        app.arg("--config")
            .arg(paths.net.join("application-config.json"))
            .arg("--genesis")
            .arg(paths.net.join("native-genesis.json"))
            .arg("--db")
            .arg(home.join("appdb"))
            .arg("--release-manifest-sha512")
            .arg(&context_hex);
        if launch.archive {
            app.args(["--block-history", "archive"]);
        }
        if launch.snapshots {
            app.arg("--snapshot-dir")
                .arg(&snapshot_dir)
                .args(["--snapshot-interval", &SNAPSHOT_INTERVAL.to_string()])
                .args(["--snapshot-keep", &SNAPSHOT_KEEP.to_string()]);
        }
        if launch.metrics {
            app.arg("--metrics-dir")
                .arg(home.join("metrics"))
                .args(["--metrics-interval-seconds", "1"]);
        }
        app.env_clear()
            .stdin(Stdio::from(app_in_read))
            .stdout(Stdio::from(app_out_write))
            .stderr(log("app")?);
        let mut children = vec![spawn_owned(owner, Role::Application, app, None)?];

        let socket = home.join("abci").join("app.sock");
        let (input, output) = (high(app_in_write.as_raw_fd())?, high(app_out_read.as_raw_fd())?);
        drop((app_in_write, app_out_read));
        let mut bridge = Command::new(paths.binary("dytallix-comet-bridge"));
        bridge
            .arg("--socket")
            .arg(format!("unix://{}", socket.display()))
            .args([
                "--application-channel=inherited-pipes-v1",
                "--application-input-fd=3",
                "--application-output-fd=4",
            ]);
        if launch.snapshots {
            bridge.arg("--snapshot-dir").arg(&snapshot_dir);
        }
        bridge.env_clear().stdin(Stdio::null()).stdout(log("bridge")?).stderr(log("bridge")?);
        children.insert(
            0,
            spawn_owned(
                owner,
                Role::Bridge,
                bridge,
                Some((input.as_raw_fd(), output.as_raw_fd())),
            )?,
        );
        drop((input, output));
        wait_for(&socket, "the bridge socket", &mut children)?;

        let mut engine = Command::new(paths.binary("dytallix-pqc-engine"));
        engine
            .args(["start", "--home"])
            .arg(&home)
            .args(["--p2p-profile", "dytallix-pqc-loopback-v1", "--rpc-profile", "dytallix-pqc-unix-v1"]);
        if let Some(dir) = launch.light_blocks {
            engine.arg("--light-blocks").arg(dir);
        }
        if launch.metrics {
            engine
                .arg("--metrics-dir")
                .arg(home.join("metrics"))
                .args(["--metrics-interval", "1s"]);
        }
        engine
            .env_clear()
            .env("HOME", &home)
            .stdin(Stdio::null())
            .stdout(log("engine")?)
            .stderr(log("engine")?);
        children.insert(0, spawn_owned(owner, Role::Engine, engine, None)?);
        wait_for(&home.join("data").join("rpc.sock"), "the engine RPC socket", &mut children)?;
        Ok(Node { index, children })
    }

    fn stop_node(paths: &Paths, node: &mut Node) -> Result<()> {
        for child in &mut node.children {
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(15);
            while child.try_wait()?.is_none() {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    child.wait()?;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        node.children.clear();
        // A later start refuses existing endpoints.
        let home = paths.home(node.index);
        for socket in [home.join("abci").join("app.sock"), home.join("data").join("rpc.sock")] {
            if socket.exists() {
                fs::remove_file(&socket)?;
            }
        }
        Ok(())
    }

    /// One GET over the engine's private Unix RPC: a 32-bit big-endian length
    /// and a JSON request, answered the same way.
    fn rpc(paths: &Paths, node: usize, path: &str, query: &str) -> Result<Value> {
        let mut stream = UnixStream::connect(paths.home(node).join("data").join("rpc.sock"))?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let request = serde_json::to_vec(&json!({"version": 1, "method": "GET", "path": path,
            "query": query, "body_base64": "", "remote_addr": "127.0.0.1:1"}))?;
        stream.write_all(&u32::try_from(request.len())?.to_be_bytes())?;
        stream.write_all(&request)?;
        let mut length = [0; 4];
        stream.read_exact(&mut length)?;
        let length = usize::try_from(u32::from_be_bytes(length))?;
        ensure!(length <= 2_097_152, "RPC response exceeds its bound");
        let mut raw = vec![0; length];
        stream.read_exact(&mut raw)?;
        let response: Value = serde_json::from_slice(&raw)?;
        ensure!(response["status"] == 200, "RPC {path} status {}", response["status"]);
        use base64::Engine;
        let body = base64::engine::general_purpose::STANDARD
            .decode(response["body_base64"].as_str().context("RPC body missing")?)?;
        let body: Value = serde_json::from_slice(&body)?;
        Ok(body["result"].clone())
    }

    struct Status {
        height: u64,
        earliest: u64,
        catching_up: bool,
    }
    fn status(paths: &Paths, node: usize) -> Result<Status> {
        let result = rpc(paths, node, "/status", "")?;
        let sync = &result["sync_info"];
        let number = |field: &str| -> Result<u64> {
            Ok(sync[field].as_str().context("Status height missing")?.parse()?)
        };
        Ok(Status {
            height: number("latest_block_height")?,
            earliest: number("earliest_block_height")?,
            catching_up: sync["catching_up"].as_bool().context("Status sync flag missing")?,
        })
    }
    /// The node's own application: its last height and application hash.
    fn app_info(paths: &Paths, node: usize) -> Result<(u64, String)> {
        use base64::Engine;
        let result = rpc(paths, node, "/abci_info", "")?;
        let response = &result["response"];
        let height = response["last_block_height"]
            .as_str()
            .context("Application height missing")?
            .parse()?;
        let hash = base64::engine::general_purpose::STANDARD.decode(
            response["last_block_app_hash"]
                .as_str()
                .context("Application hash missing")?,
        )?;
        Ok((height, hex::encode(hash)))
    }
    /// The chain's application hash after `height`, from the header at
    /// height + 1.
    fn app_hash_after(paths: &Paths, node: usize, height: u64) -> Result<String> {
        let result = rpc(paths, node, "/block", &format!("height={}", height + 1))?;
        Ok(result["block"]["header"]["app_hash"]
            .as_str()
            .context("Header application hash missing")?
            .to_lowercase())
    }

    fn wait_height(paths: &Paths, node: usize, height: u64, timeout: Duration) -> Result<Status> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(status) = status(paths, node) {
                if status.height >= height && !status.catching_up {
                    return Ok(status);
                }
            }
            ensure!(Instant::now() < deadline, "Node {node} did not reach height {height}");
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The height node 3's engine restored a snapshot at, from its log: the
    /// engine's record that the application passed the trusted checks.
    fn restored_height(paths: &Paths) -> Result<Option<u64>> {
        let log = fs::read_to_string(paths.logs.join("node3-engine.log"))?;
        Ok(log
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|record| record["_msg"] == "Snapshot restored" && record["module"] == "statesync")
            .and_then(|record| record["height"].as_u64()))
    }

    /// Node 0's metrics files (metrics v1): the core values a running chain
    /// must show, each positive, and write times within the last minute.
    fn metrics_values(paths: &Paths) -> Result<Value> {
        let dir = paths.home(0).join("metrics");
        let read = |file: &str, name: &str| -> Result<f64> {
            let text = fs::read_to_string(dir.join(file))
                .with_context(|| format!("Metrics file {file} missing"))?;
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{name} ")))
                .with_context(|| format!("{name} missing from {file}"))?
                .parse()
                .with_context(|| format!("{name} is not a number"))
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs_f64();
        let mut values = serde_json::Map::new();
        for (file, name) in [
            ("dytallix-engine.prom", "dytallix_engine_consensus_height"),
            ("dytallix-engine.prom", "dytallix_engine_p2p_peers"),
            ("dytallix-engine.prom", "dytallix_engine_consensus_validators"),
            ("dytallix-app.prom", "dytallix_app_height"),
            ("dytallix-app.prom", "dytallix_app_commit_seconds_count"),
            ("dytallix-app.prom", "dytallix_app_snapshot_latest_height"),
            ("dytallix-app.prom", "dytallix_app_supply_udrt{bucket=\"total\"}"),
        ] {
            let value = read(file, name)?;
            ensure!(value > 0.0, "{name} is {value} on a running chain");
            values.insert(name.into(), json!(value));
        }
        for (file, process) in [("dytallix-engine.prom", "engine"), ("dytallix-app.prom", "app")] {
            let written =
                read(file, &format!("dytallix_metrics_written_timestamp_seconds{{process=\"{process}\"}}"))?;
            ensure!((now - written).abs() < 60.0, "{file} is stale by {} s", now - written);
        }
        Ok(Value::Object(values))
    }

    /// Node 3 syncs from the snapshot: state sync on, trusting TRUST_HEIGHT
    /// and its hash, with no RPC servers.
    fn enable_state_sync(paths: &Paths, trust_hash: &str) -> Result<()> {
        let path = paths.home(3).join("config").join("config.toml");
        let mut config: toml::Table = toml::from_str(&fs::read_to_string(&path)?)?;
        let sync = config
            .get_mut("statesync")
            .and_then(toml::Value::as_table_mut)
            .context("Engine configuration has no statesync table")?;
        sync.insert("enable".into(), toml::Value::Boolean(true));
        sync.insert("rpc_servers".into(), toml::Value::String(String::new()));
        sync.insert("trust_height".into(), toml::Value::Integer(TRUST_HEIGHT as i64));
        sync.insert("trust_hash".into(), toml::Value::String(trust_hash.to_owned()));
        sync.insert("trust_period".into(), toml::Value::String(TRUST_PERIOD.into()));
        fs::write(&path, toml::to_string(&config)?)?;
        Ok(())
    }

    pub fn run() -> Result<String> {
        check_confinement()?;
        let owner = OwnerThread::new()?;
        let (bin, work) = arguments()?;
        private_dir(&work)?;
        let paths = Paths {
            net: work.join("net"),
            logs: work.join("logs"),
            light: work.join("light"),
            bin,
            work,
        };
        private_dir(&paths.logs)?;
        let genesis = paths.work.join("native-genesis.json");
        private_file(&genesis, &native_genesis()?)?;
        let now = chrono_now()?;
        run_tool(&paths, "dytallix-comet-fixture", &[
            "--output", path_str(&paths.net)?, "--app-genesis", path_str(&genesis)?,
            "--chain-id", CHAIN, "--genesis-time", &now, "--base-port", &BASE_PORT.to_string(),
            "--p2p-profile", "dytallix-pqc-loopback-v1",
        ])?;
        for node in 0..4 {
            private_dir(&paths.home(node).join("snapshots"))?;
            private_dir(&paths.home(node).join("metrics"))?;
        }

        let committing = |index: usize| Launch {
            archive: index == 2,
            snapshots: true,
            light_blocks: None,
            metrics: index == 0,
        };
        let mut nodes = Vec::new();
        for index in 0..3 {
            nodes.push(start_node(&owner, &paths, index, &committing(index))?);
        }
        // Export light blocks off a stopped node 2, up to its own head, then
        // restart it. The chain waits for it: three of four validators commit.
        let exported = wait_height(&paths, 2, EXPORT_AFTER, STEP_TIMEOUT)?.height;
        stop_node(&paths, &mut nodes[2])?;
        let summary = run_tool(&paths, "dytallix-light-export", &[
            "--home", path_str(&paths.home(2))?, "--from", &TRUST_HEIGHT.to_string(),
            "--to", &exported.to_string(), "--output", path_str(&paths.light)?,
        ])?;
        let summary: Value = serde_json::from_slice(&summary)?;
        let trust_hash = summary["trust_hash"]
            .as_str()
            .context("Export printed no trust hash")?
            .to_owned();
        nodes[2] = start_node(&owner, &paths, 2, &committing(2))?;

        enable_state_sync(&paths, &trust_hash)?;
        nodes.push(start_node(&owner, &paths, 3, &Launch {
            archive: false,
            snapshots: false,
            light_blocks: Some(&paths.light),
            metrics: false,
        })?);
        let synced = wait_height(&paths, 3, exported + 3, JOIN_TIMEOUT)?;
        // Node 3 follows the chain: its own application reaches a later
        // height with the chain's application hash for it.
        wait_height(&paths, 3, synced.height + 3, STEP_TIMEOUT)?;
        let (height, app_hash) = app_info(&paths, 3)?;
        wait_height(&paths, 0, height + 1, STEP_TIMEOUT)?;
        let reference = app_hash_after(&paths, 0, height)?;
        let restored = restored_height(&paths)?;
        let metrics = metrics_values(&paths)?;
        let result = json!({
            "chain_id": CHAIN,
            "export": {"from": TRUST_HEIGHT, "to": exported, "trust_hash": trust_hash},
            "node3": {"restored_snapshot": restored, "earliest_block": synced.earliest,
                "synced_height": synced.height, "application_height": height,
                "application_hash": app_hash},
            "chain_app_hash": reference,
            "node0_metrics": metrics,
        });
        for node in nodes.iter_mut().rev() {
            stop_node(&paths, node)?;
        }
        // A snapshot within the exported light blocks, and blocks after it.
        let restored = restored.context("Node 3 restored no snapshot")?;
        ensure!(
            (TRUST_HEIGHT..=exported.saturating_sub(2)).contains(&restored)
                && synced.earliest > restored
                && height > restored,
            "Node 3 restored height {restored} outside the export or did not follow it"
        );
        ensure!(
            app_hash == reference,
            "Node 3 application hash {app_hash} differs from the chain's {reference}"
        );
        Ok(serde_json::to_string(&result)?)
    }

    fn path_str(path: &Path) -> Result<&str> {
        path.to_str().context("Path is not UTF-8")
    }

    /// RFC 3339 UTC time now, from the system clock.
    fn chrono_now() -> Result<String> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Ok(format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        ))
    }
}
