use axum::{
    http::{header, HeaderValue, Method},
    routing::{get, post},
    Extension, Router,
};
use dotenv::dotenv;
use serde_json::json;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::interval;
use tower_http::cors::CorsLayer;

// Replace crate:: module imports with library crate path so binary can access lib modules
use dytallix_fast_node::alerts::{load_alerts_config, AlertsEngine, NodeMetricsGatherer};
use dytallix_fast_node::mempool::Mempool;
use dytallix_fast_node::metrics::{parse_metrics_config, MetricsServer};
use dytallix_fast_node::rpc::{self, RpcContext};
use dytallix_fast_node::runtime::bridge; // import bridge module for validator init
use dytallix_fast_node::runtime::emission::EmissionEngine;
use dytallix_fast_node::runtime::fee_burn::FeeBurnEngine;
use dytallix_fast_node::runtime::governance::GovernanceConfig;
use dytallix_fast_node::runtime::governance::GovernanceModule;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::secrets; // validator key providers (Vault / sealed keystore)
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::{blocks::TpsWindow, state::Storage};
use dytallix_fast_node::ws::server::{ws_handler, WsHub};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    // Fail closed: refuse to start if PQC verification is not compiled in.
    // (pqc-mock is intentionally not accepted for runtime node operation.)
    #[cfg(all(not(feature = "pqc-real"), not(feature = "pqc-fips204")))]
    {
        eprintln!(
            "FATAL: node built without PQC verification (enable feature pqc-fips204 or pqc-real; default features include pqc-fips204)"
        );
        std::process::exit(1);
    }

    let data_dir = std::env::var("DYT_DATA_DIR").unwrap_or("./data".to_string());
    let block_interval_ms: u64 = std::env::var("DYT_BLOCK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15000);
    let slots_per_epoch: u64 = std::env::var("DYT_SLOTS_PER_EPOCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            let seconds_per_block = (block_interval_ms / 1000).max(1);
            (86_400 / seconds_per_block).max(1)
        });
    let empty_blocks = std::env::var("DYT_EMPTY_BLOCKS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);
    let max_txs: usize = std::env::var("BLOCK_MAX_TX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let ws_enabled = std::env::var("DYT_WS_ENABLED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);
    let chain_id = std::env::var("DYT_CHAIN_ID").unwrap_or("dyt-local-1".to_string());

    // Runtime feature flags (default disabled) - moved up to fix compilation
    let enable_governance = std::env::var("DYT_ENABLE_GOVERNANCE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    let enable_staking = std::env::var("DYT_ENABLE_STAKING")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let dev_endpoints_requested = std::env::var("DYT_ENABLE_DEV_ENDPOINTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    // LR01 rejects the retired timer before directory or database writes.
    // Diagnostic fixture features never reopen this supported entrypoint.
    dytallix_fast_node::block_settlement::check_profile(
        std::env::var("DYT_BLOCK_PROFILE").ok().as_deref(),
        enable_governance,
        dev_endpoints_requested,
    )?;
    std::fs::create_dir_all(&data_dir)?;
    let genesis_bytes = match std::fs::read("genesis.json") {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let genesis_json: Option<serde_json::Value> = genesis_bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?;
    anyhow::ensure!(
        genesis_json
            .as_ref()
            .and_then(|value| value
                .get("reward_v2")
                .or_else(|| value.get("adaptive_issuance")))
            .is_none(),
        "Reward-v2 requires a finalization adapter. The node timer cannot activate this policy."
    );
    let mut storage = Storage::open(PathBuf::from(format!("{data_dir}/node.db")))?;
    anyhow::ensure!(
        storage
            .db
            .iterator(rocksdb::IteratorMode::From(
                b"consensus:",
                rocksdb::Direction::Forward
            ))
            .next()
            .transpose()?
            .is_none_or(|(key, _)| !key.starts_with(b"consensus:")),
        "Consensus state cannot run under the node timer."
    );
    anyhow::ensure!(
        storage
            .db
            .get(dytallix_fast_node::runtime::reward_runtime::REWARD_STATE_KEY)?
            .is_none(),
        "Reward-v2 state cannot run under the node timer."
    );
    anyhow::ensure!(
        storage
            .db
            .get(dytallix_fast_node::runtime::issuance_timing::TIMING_STATE_KEY)?
            .is_none(),
        "Issuance timing state requires the explicit development finalization adapter."
    );
    dytallix_fast_node::genesis::initialize(&mut storage, &chain_id, genesis_bytes.as_deref())?;
    dytallix_fast_node::block_settlement::verify_recovery(&storage)?;
    let storage = Arc::new(storage);
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking_module = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let emission_config = match std::env::var("DYT_EMISSION_CONFIG") {
        Ok(path) => Some(serde_json::from_slice::<
            dytallix_fast_node::runtime::emission::EmissionConfig,
        >(&std::fs::read(path)?)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let emission = Arc::new(Mutex::new(match emission_config {
        Some(config) => EmissionEngine::new_with_config(storage.clone(), state.clone(), config),
        None => {
            let mut engine = EmissionEngine::new(storage.clone(), state.clone());
            engine.config.initial_supply = dytallix_fast_node::supply::inspect(&storage)?.genesis;
            engine
        }
    }));
    dytallix_fast_node::block_settlement::verify_policy(&emission.lock().unwrap(), enable_staking)?;

    // Build governance config from ENV first, then fall back to genesis.json, finally defaults
    let mut gov_cfg = GovernanceConfig::default();
    // Helper closures
    let parse_u64 = |k: &str| -> Option<u64> { std::env::var(k).ok().and_then(|v| v.parse().ok()) };
    let parse_u128 =
        |k: &str| -> Option<u128> { std::env::var(k).ok().and_then(|v| v.parse().ok()) };

    if let Some(v) = parse_u128("DYT_GOV_MIN_DEPOSIT") {
        gov_cfg.min_deposit = v;
    }
    if let Some(v) = parse_u64("DYT_GOV_DEPOSIT_PERIOD") {
        gov_cfg.deposit_period = v;
    }
    if let Some(v) = parse_u64("DYT_GOV_VOTING_PERIOD") {
        gov_cfg.voting_period = v;
    }
    if let Some(v) = parse_u128("DYT_GOV_QUORUM_BPS") {
        gov_cfg.quorum = v;
    }
    if let Some(v) = parse_u128("DYT_GOV_THRESHOLD_BPS") {
        gov_cfg.threshold = v;
    }
    if let Some(v) = parse_u128("DYT_GOV_VETO_BPS") {
        gov_cfg.veto_threshold = v;
    }

    // Genesis fallbacks (only if env not set / still default)
    if let Some(genesis) = genesis_json.as_ref() {
        if let Some(gov) = genesis.get("governance") {
            if gov_cfg.min_deposit == GovernanceConfig::default().min_deposit {
                if let Some(v) = gov
                    .get("min_deposit_udgt")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u128>().ok())
                {
                    gov_cfg.min_deposit = v;
                }
            }
            if gov_cfg.deposit_period == GovernanceConfig::default().deposit_period {
                if let Some(v) = gov.get("deposit_period").and_then(|v| v.as_u64()) {
                    gov_cfg.deposit_period = v;
                }
            }
            if gov_cfg.voting_period == GovernanceConfig::default().voting_period {
                if let Some(v) = gov.get("voting_period").and_then(|v| v.as_u64()) {
                    gov_cfg.voting_period = v;
                }
            }
            if gov_cfg.quorum == GovernanceConfig::default().quorum {
                if let Some(v) = gov
                    .get("quorum_bps")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u128>().ok())
                {
                    gov_cfg.quorum = v;
                }
            }
            if gov_cfg.threshold == GovernanceConfig::default().threshold {
                if let Some(v) = gov
                    .get("threshold_bps")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u128>().ok())
                {
                    gov_cfg.threshold = v;
                }
            }
            if gov_cfg.veto_threshold == GovernanceConfig::default().veto_threshold {
                if let Some(v) = gov
                    .get("veto_threshold_bps")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u128>().ok())
                {
                    gov_cfg.veto_threshold = v;
                }
            }
        }
    }
    println!("GovernanceConfig:min_deposit={} deposit_period={} voting_period={} quorum_bps={} threshold_bps={} veto_bps={} ", gov_cfg.min_deposit, gov_cfg.deposit_period, gov_cfg.voting_period, gov_cfg.quorum, gov_cfg.threshold, gov_cfg.veto_threshold);

    let mempool = Arc::new(Mutex::new(Mempool::new()));
    let fee_burn_engine = Arc::new(Mutex::new(FeeBurnEngine::new()));
    let ws_hub = WsHub::new();
    let tps_window = Arc::new(Mutex::new(TpsWindow::new(60)));

    // Decide and log secrets mode (without leaking secrets)
    let vault_url = std::env::var("DYTALLIX_VAULT_URL")
        .ok()
        .or_else(|| std::env::var("VAULT_URL").ok());
    let vault_token_present =
        std::env::var("DYTALLIX_VAULT_TOKEN").is_ok() || std::env::var("VAULT_TOKEN").is_ok();
    if let (Some(url), true) = (vault_url.clone(), vault_token_present) {
        let mount =
            std::env::var("DYTALLIX_VAULT_KV_MOUNT").unwrap_or_else(|_| "secret".to_string());
        let base = std::env::var("DYTALLIX_VAULT_PATH_BASE")
            .unwrap_or_else(|_| "dytallix/validators".to_string());
        // Redact token; show only host portion of URL
        let host = url
            .split("//")
            .nth(1)
            .unwrap_or(&url)
            .split('/')
            .next()
            .unwrap_or(&url);
        println!("Secrets mode: Vault (KV v2) url_host={host} mount={mount} base={base}");
    } else {
        let dir = std::env::var("DYT_KEYSTORE_DIR").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.dytallix/keystore")
        });
        println!("Secrets mode: Plain Keystore (dev) path={dir} — no passphrase required");
    }

    // Load validator private key securely (Vault preferred, sealed keystore fallback)
    match secrets::init_validator_key(
        dytallix_fast_node::addr::AddressNetwork::Development,
        &chain_id,
    )
    .await
    {
        Ok(Some(len)) => {
            println!("Validator key loaded ({len} bytes) via secure provider");
        }
        Ok(None) => {
            println!("No validator key configured; running without signing capability");
        }
        Err(e) => {
            eprintln!("Validator key initialization failed: {e}");
            // Stop on key errors in existing-key mode or when Vault is configured.
            if secrets::require_existing_validator_key()
                || std::env::var("DYTALLIX_VAULT_URL").is_ok()
                || std::env::var("VAULT_URL").is_ok()
            {
                std::process::exit(1);
            }
        }
    }
    if let Some(address) = secrets::validator_address() {
        println!("Validator proposer address: {address}");
    }
    println!("Chain slots per epoch: {slots_per_epoch}");

    // Initialize metrics
    let metrics_config = parse_metrics_config();
    let (metrics_server, metrics) = MetricsServer::new(metrics_config.clone())?;

    // Initialize alerting system
    let alerts_config_path =
        std::env::var("DYT_ALERTS_CONFIG").unwrap_or_else(|_| "./configs/alerts.yaml".to_string());
    let alerts_config = load_alerts_config(std::path::Path::new(&alerts_config_path))?;

    #[cfg(feature = "metrics")]
    let mut alerts_engine = {
        // Create a dummy registry for now - in a real implementation this would be shared
        let alerts_registry = prometheus::Registry::new();
        AlertsEngine::new(alerts_config.clone(), &alerts_registry)?
    };

    #[cfg(not(feature = "metrics"))]
    let mut alerts_engine = AlertsEngine::new(alerts_config.clone())?;

    // Replace previous ctx creation to use custom governance config
    let ctx = RpcContext {
        storage: storage.clone(),
        mempool: mempool.clone(),
        state: state.clone(),
        ws: ws_hub.clone(),
        tps: tps_window.clone(),
        emission: emission.clone(),
        governance: Arc::new(Mutex::new(
            GovernanceModule::open(
                storage.clone(),
                state.clone(),
                staking_module.clone(),
                enable_governance,
            )
            .map_err(anyhow::Error::msg)?,
        )),
        staking: staking_module.clone(),
        metrics: metrics.clone(),
        fee_burn: fee_burn_engine.clone(),
        features: dytallix_fast_node::rpc::FeatureFlags {
            governance: enable_governance,
            staking: enable_staking,
        },
        wasm_contracts: Arc::new(Mutex::new(std::collections::HashMap::new())),
        #[cfg(feature = "contracts")]
        wasm_runtime: Arc::new(dytallix_fast_node::runtime::wasm::WasmRuntime::new()),
        pending_assets: Arc::new(Mutex::new(Vec::new())),
        proposer_address: secrets::validator_address().map(str::to_owned),
        validator_public_key_b64: secrets::validator_public_key_b64(),
        validator_algorithm: secrets::validator_algorithm().map(str::to_owned),
        slots_per_epoch,
    };

    // Initialize bridge validators if provided
    bridge::ensure_bridge_validators(&storage.db).ok();

    // Block producer task
    let producer_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(block_interval_ms));
        loop {
            ticker.tick().await;
            // Allow ops to pause block production to simulate stalls
            if dytallix_fast_node::production_control::PRODUCTION.state()
                != dytallix_fast_node::production_control::ProductionState::Running
            {
                continue;
            }
            let block_start_time = SystemTime::now();

            // Update mempool size metric
            let mempool_size = { producer_ctx.mempool.lock().unwrap().len() };
            producer_ctx.metrics.update_mempool_size(mempool_size);

            let snapshot = {
                let state = producer_ctx.state.lock().unwrap();
                let mut pool = producer_ctx.mempool.lock().unwrap();
                if let Err(error) = pool.reconcile(&state, &[]) {
                    dytallix_fast_node::production_control::PRODUCTION.fail();
                    tracing::error!(%error,"Queue reconciliation failed; production stopped");
                    return;
                }
                pool.take_snapshot(max_txs)
            };
            let assets = { producer_ctx.pending_assets.lock().unwrap().clone() };
            let ts = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(time) => time.as_secs(),
                Err(error) => {
                    dytallix_fast_node::production_control::PRODUCTION.fail();
                    tracing::error!(%error, "Invalid block clock; production stopped");
                    return;
                }
            };
            let result = {
                // Fixed order also agrees with existing read-side module locks.
                let mut emission = producer_ctx.emission.lock().unwrap();
                let mut staking = producer_ctx.staking.lock().unwrap();
                let mut state = producer_ctx.state.lock().unwrap();
                let mut burn = producer_ctx.fee_burn.lock().unwrap();
                dytallix_fast_node::block_settlement::commit_block(
                    &mut state,
                    &mut emission,
                    &mut staking,
                    &mut burn,
                    producer_ctx.features.staking,
                    &dytallix_fast_node::block_settlement::BlockRequest {
                        height: producer_ctx.storage.height().saturating_add(1),
                        transactions: &snapshot,
                        assets: &assets,
                        timestamp: ts,
                        empty_blocks,
                    },
                )
            };
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    dytallix_fast_node::production_control::PRODUCTION.fail();
                    tracing::error!(%error, "Block settlement failed; production stopped; recovery required");
                    return;
                }
            };
            for elapsed in &outcome.transaction_processing_times {
                producer_ctx.metrics.record_transaction(*elapsed);
            }
            // Pre-fee rejections have no block position or durable fee effects.
            // Remove considered entries only after a successful planning/commit result.
            {
                let state = producer_ctx.state.lock().unwrap();
                if let Err(error) = producer_ctx.mempool.lock().unwrap().reconcile(
                    &state,
                    &snapshot
                        .iter()
                        .map(|tx| tx.hash.clone())
                        .collect::<Vec<_>>(),
                ) {
                    dytallix_fast_node::production_control::PRODUCTION.fail();
                    tracing::error!(%error,"Committed queue reconciliation failed; production stopped");
                    return;
                }
            }
            let Some(block) = outcome.block else {
                continue;
            };
            if let Err(error) = dytallix_fast_node::block_settlement::acknowledge_assets(
                &mut producer_ctx.pending_assets.lock().unwrap(),
                &assets,
            ) {
                dytallix_fast_node::production_control::PRODUCTION.fail();
                tracing::error!(%error, "Block committed but asset queue changed; production stopped");
                return;
            }
            let height = block.header.height;
            let total_gas_used = outcome.gas_used;

            // Record metrics
            if let Ok(block_processing_time) = block_start_time.elapsed() {
                producer_ctx.metrics.record_block(
                    height,
                    block.txs.len(),
                    total_gas_used,
                    block_processing_time,
                );
            }
            producer_ctx
                .metrics
                .update_current_block_gas(total_gas_used);

            // Update emission pool metrics
            let emission_snapshot = producer_ctx.emission.lock().unwrap().snapshot();
            let total_emission_pool: u128 = emission_snapshot.pools.values().sum();
            producer_ctx
                .metrics
                .update_emission_pool(total_emission_pool as f64);
            // Update emissions ops metrics (height, pending uDRT total, last apply ts)
            let now_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs();
            producer_ctx.metrics.update_emission_apply(
                emission_snapshot.height,
                total_emission_pool,
                now_ts,
            );

            producer_ctx
                .tps
                .lock()
                .unwrap()
                .record_block(ts, block.txs.len() as u32);
            // Update TPS gauge when metrics are enabled
            #[cfg(feature = "metrics")]
            {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_else(|_| Duration::from_secs(0))
                    .as_secs();
                let tps = producer_ctx.tps.lock().unwrap().rolling_tps(now);
                producer_ctx.metrics.dyt_tps.set(tps);
                producer_ctx.metrics.tps.set(tps);
            }
            if ws_enabled {
                producer_ctx.ws.broadcast_json(&json!({"type":"new_block","height": block.header.height, "hash": block.hash, "txs": block.txs.iter().map(|t| &t.hash).collect::<Vec<_>>() }));
            }
            println!(
                "produced block height={} considered={} included={}",
                block.header.height,
                snapshot.len(),
                block.txs.len()
            );
        }
    });

    // Router
    let mut app = Router::new()
        .route("/submit", post(rpc::submit))
        .route("/transactions/submit", post(rpc::submit)) // Standard endpoint path
        .route("/blocks", get(rpc::list_blocks))
        .route("/api/blocks", get(rpc::list_blocks)) // API prefix version
        .route("/api/anchored-assets", get(rpc::list_anchored_assets)) // All blocks with anchored assets (no limit)
        .route("/block/:id", get(rpc::get_block))
        .route("/balance/:addr", get(rpc::get_balance))
        .route("/account/:addr", get(rpc::get_account))
        .route("/tx/:hash", get(rpc::get_tx))
        .route("/transactions", get(rpc::list_transactions)) // List all transactions
        .route("/transactions/:hash", get(rpc::get_tx)) // Standard endpoint path
        .route("/transactions/pending", get(rpc::get_pending_transactions)) // Pending transactions list
        .route("/genesis", get(rpc::get_genesis)) // Genesis configuration
        .route("/genesis/hash", get(rpc::get_genesis_hash)) // Genesis hash
        // Minimal JSON-RPC endpoint used by the dashboard server for WASM demos
        .route("/rpc", post(rpc::json_rpc))
        .route("/metrics", get(rpc::metrics_export))
        .route("/stats", get(rpc::stats))
        .route("/capabilities", get(rpc::public_capabilities))
        .route("/api/capabilities", get(rpc::public_capabilities))
        .route("/status", get(rpc::status))
        .route("/health", get(rpc::health))
        .route("/peers", get(rpc::peers))
        .route("/bridge/ingest", post(rpc::bridge_ingest))
        .route("/bridge/halt", post(rpc::bridge_halt))
        .route("/bridge/state", get(rpc::bridge_state))
        .route("/emission/claim", post(rpc::emission_claim))
        .route("/api/rewards", get(rpc::get_rewards))
        .route("/api/rewards/:height", get(rpc::get_rewards_by_height))
        .route("/api/stats", get(rpc::stats_with_emission))
        .route("/api/contracts", get(rpc::list_contracts))
        .route(
            "/params/staking_reward_rate",
            get(rpc::params_staking_reward_rate),
        )
        // Asset Registry endpoints
        .route("/asset/register", post(rpc::asset_register))
        .route("/asset/verify", post(rpc::asset_verify))
        .route("/asset/get", post(rpc::asset_get));

    // Dev/ops endpoints credit balances directly (/dev/faucet) and pause/resume
    // block production (/ops/*). They are UNAUTHENTICATED, so exposing them on a
    // public node lets anyone mint arbitrary balances or halt the chain. They are
    // therefore disabled unless DYT_ENABLE_DEV_ENDPOINTS=true is explicitly set
    // (intended for local end-to-end testing only).
    let dev_endpoints_enabled = std::env::var("DYT_ENABLE_DEV_ENDPOINTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
    if dev_endpoints_enabled {
        eprintln!(
            "[WARN] Dev endpoints ENABLED (DYT_ENABLE_DEV_ENDPOINTS=true): \
             /dev/faucet, /ops/pause and /ops/resume are unauthenticated. \
             Never enable this on a public or production node."
        );
        app = app
            .route("/dev/faucet", post(rpc::dev_faucet))
            .route("/ops/pause", post(rpc::ops_pause))
            .route("/ops/resume", post(rpc::ops_resume));
    }

    // WASM contract routes
    #[cfg(feature = "contracts")]
    {
        app = app
            .route("/api/contracts/:contract_address", get(rpc::contract_info))
            .route(
                "/api/contracts/:contract_address/query/:method",
                get(rpc::contract_query),
            )
            .route(
                "/api/contracts/:contract_address/events",
                get(rpc::contract_events),
            )
            .route(
                "/api/contracts/:contract_address/state/:key",
                get(rpc::contracts_state),
            )
            .route("/contracts/deploy", post(rpc::contracts_deploy))
            .route("/contracts/call", post(rpc::contracts_call))
            .route(
                "/contracts/state/:contract_address/:key",
                get(rpc::contracts_state),
            );
    }

    // Oracle routes
    #[cfg(feature = "oracle")]
    {
        app = app
            .route("/ai/score", post(rpc::ai::ai_score))
            .route("/ai/risk/:hash", get(rpc::ai::ai_risk_get))
            .route("/ai/latency", get(rpc::ai::ai_latency))
            .route("/oracle/ai_risk", post(rpc::oracle::submit_ai_risk))
            .route(
                "/oracle/ai_risk_batch",
                post(rpc::oracle::submit_ai_risk_batch),
            )
            .route(
                "/oracle/ai_risk_query_batch",
                post(rpc::oracle::get_ai_risk_batch),
            )
            .route("/oracle/stats", get(rpc::oracle::oracle_stats));
    }

    // Governance routes (always exposed; tx endpoints return 501 if disabled)
    app = app
        .route("/gov/submit", post(rpc::gov_submit_proposal))
        .route("/gov/deposit", post(rpc::gov_deposit))
        .route("/gov/vote", post(rpc::gov_vote))
        .route("/gov/execute", post(rpc::gov_execute))
        .route("/gov/proposal/:id", get(rpc::gov_get_proposal))
        .route("/gov/tally/:id", get(rpc::gov_tally))
        .route("/gov/config", get(rpc::gov_get_config))
        .route("/api/governance/proposals", get(rpc::gov_list_proposals))
        .route(
            "/api/governance/proposals/:id/votes",
            get(rpc::gov_get_proposal_votes),
        )
        .route(
            "/api/governance/voting-power/:address",
            get(rpc::gov_get_voting_power),
        )
        .route(
            "/api/governance/total-voting-power",
            get(rpc::gov_get_total_voting_power),
        );

    app = app
        .route(
            "/api/transactions/:hash/record",
            get(rpc::get_transaction_record),
        )
        .route("/api/supply/drt", get(rpc::drt_supply))
        .route("/api/supply/dgt", get(rpc::dgt_supply));

    // Staking routes (always exposed; tx endpoints return 501 if disabled)
    app = app
        .route("/api/staking/claim", post(rpc::staking_claim))
        .route("/api/staking/delegate", post(rpc::staking_delegate))
        .route("/api/staking/undelegate", post(rpc::staking_undelegate))
        .route(
            "/api/staking/accrued/:address",
            get(rpc::staking_get_accrued),
        )
        .route(
            "/api/staking/balance/:delegator",
            get(rpc::staking_get_balance),
        )
        .route("/api/staking/stats", get(rpc::staking_get_stats))
        .route("/api/staking/validators", get(rpc::staking_get_validators));

    // API routes for explorer search (aliases with /api prefix)
    app = app
        .route("/api/block/:id", get(rpc::get_block))
        .route("/api/tx/:hash", get(rpc::get_tx))
        .route("/api/balance/:addr", get(rpc::get_balance));

    app = app.layer(Extension(ctx));

    // Add CORS middleware. Origins are restricted to an explicit allow-list
    // (DYT_CORS_ORIGINS, comma-separated) rather than reflecting any origin, so
    // untrusted web pages cannot drive the node's state-mutating endpoints from a
    // visitor's browser. Defaults to local dev origins when unset.
    let cors_origins: Vec<HeaderValue> = std::env::var("DYT_CORS_ORIGINS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<HeaderValue>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            vec![
                HeaderValue::from_static("http://localhost:3000"),
                HeaderValue::from_static("http://127.0.0.1:3000"),
            ]
        });
    app = app.layer(
        CorsLayer::new()
            .allow_origin(cors_origins)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
    );

    if ws_enabled {
        app = app.route("/ws", get(ws_handler).layer(Extension(ws_hub)));
    }

    // Start metrics server if enabled
    if metrics_config.enabled {
        let metrics_server_task = tokio::spawn(async move {
            if let Err(e) = metrics_server.start().await {
                eprintln!("Metrics server error: {e}");
            }
        });

        // Don't wait for metrics server, let it run in background
        std::mem::forget(metrics_server_task);
    }

    // Start alerts engine if enabled
    if alerts_config.enabled {
        let metrics_gatherer = Arc::new(NodeMetricsGatherer::new(tps_window.clone()));
        let alerts_task = tokio::spawn(async move {
            if let Err(e) = alerts_engine.start(metrics_gatherer).await {
                eprintln!("Alerts engine error: {e}");
            }
        });

        // Don't wait for alerts engine, let it run in background
        std::mem::forget(alerts_task);
    }

    let rpc_port: u16 = std::env::var("DYT_RPC_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3030);
    let addr: SocketAddr = format!("0.0.0.0:{}", rpc_port).parse().unwrap();
    println!("Node listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
