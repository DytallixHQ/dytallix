use axum::{
    routing::{get, post},
    Extension, Router,
};
use dytallix_fast_node::{
    mempool::Mempool, rpc, runtime::emission::EmissionEngine, runtime::fee_burn::FeeBurnEngine,
    runtime::governance::GovernanceModule, runtime::staking::StakingModule, state::State,
    storage::blocks::TpsWindow, storage::state::Storage, ws::server::WsHub,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

fn app() -> (
    Router,
    dytallix_fast_node::rpc::RpcContext,
    tempfile::TempDir,
) {
    app_for_owner("alice", false)
}

fn app_for_owner(owner: &str, reward_v2: bool) -> (Router, rpc::RpcContext, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
    let mut source = json!({"chain_id":"admission-local","accounts":[{"address":owner,
        "balances":{"udgt":"10000000","udrt":"100000000"}, "vesting":{"kind":"unlocked"}}]});
    if reward_v2 {
        source["reward_v2"] = json!({"version":2,"activation_height":1,"decimals":6,"profile":"development",
            "max_validators":4,"max_positions":8,"validators":[],"positions":[]});
    }
    dytallix_fast_node::genesis::initialize(
        &mut storage,
        "admission-local",
        Some(&serde_json::to_vec(&source).unwrap()),
    )
    .unwrap();
    let storage = Arc::new(storage);
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let mempool = Arc::new(Mutex::new(Mempool::new()));
    let tps = Arc::new(Mutex::new(TpsWindow::new(60)));
    let ws = WsHub::new();
    let emission = Arc::new(Mutex::new(EmissionEngine::new(
        storage.clone(),
        state.clone(),
    )));
    emission.lock().unwrap().config.initial_supply = 100_000_000;
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = Arc::new(Mutex::new(GovernanceModule::new(
        storage.clone(),
        state.clone(),
        staking.clone(),
    )));
    let metrics = Arc::new(dytallix_fast_node::metrics::Metrics::new().expect("metrics"));
    let ctx = dytallix_fast_node::rpc::RpcContext {
        storage,
        mempool,
        state,
        ws,
        tps,
        emission,
        governance,
        staking,
        metrics,
        fee_burn: Arc::new(Mutex::new(FeeBurnEngine::new())),
        features: dytallix_fast_node::rpc::FeatureFlags {
            governance: false,
            staking: true,
        },
        // Add minimal wasm contracts map required by RpcContext
        wasm_contracts: Arc::new(Mutex::new(std::collections::HashMap::new())),
        #[cfg(feature = "contracts")]
        wasm_runtime: Arc::new(dytallix_fast_node::runtime::wasm::WasmRuntime::new()),
        pending_assets: Arc::new(Mutex::new(Vec::new())),
        proposer_address: None,
        validator_public_key_b64: None,
        validator_algorithm: None,
        slots_per_epoch: 100,
    };
    let router = Router::new()
        .route("/submit", post(rpc::submit))
        .route(
            "/api/transactions/:hash/record",
            get(rpc::get_transaction_record),
        )
        .route("/stats", get(rpc::stats))
        .route("/balance/:addr", get(rpc::get_balance))
        .route("/emission/claim", post(rpc::emission_claim))
        .route("/api/staking/delegate", post(rpc::staking_delegate))
        .route("/api/staking/undelegate", post(rpc::staking_undelegate))
        .route("/api/staking/claim", post(rpc::staking_claim))
        .layer(Extension(ctx.clone()));
    (router, ctx, dir)
}

fn signed(denom: &str, amount: u128, nonce: u64) -> dytallix_fast_node::types::tx::SignedTx {
    use dytallix_fast_node::{
        crypto::{ActivePQC, PQC},
        types::tx::{Msg, SignedTx, Tx},
    };
    let (sk, pk) = ActivePQC::keypair();
    SignedTx::sign(
        Tx {
            chain_id: "admission-local".into(),
            nonce,
            msgs: vec![Msg::Send {
                from: "alice".into(),
                to: "receiver".into(),
                denom: denom.into(),
                amount,
            }],
            fee: 21_000_000,
            memo: String::new(),
        },
        &sk,
        &pk,
    )
    .unwrap()
}
fn fund(ctx: &rpc::RpcContext) {
    ctx.storage.set_chain_id("admission-local").unwrap();
    let mut state = ctx.state.lock().unwrap();
    state.set_balance("alice", "udgt", 10_000_000);
    state.set_balance("alice", "udrt", 100_000_000);
}
async fn submit(
    app: &Router,
    tx: dytallix_fast_node::types::tx::SignedTx,
) -> axum::http::StatusCode {
    app.clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/submit")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({"signed_tx":tx})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
#[tokio::test]
async fn public_aliases_convert_amount_and_denomination_together() {
    for (alias, denom, amount) in [
        ("DGT", "udgt", 2_000_000),
        ("dRt", "udrt", 2_000_000),
        ("UDGT", "udgt", 2),
        ("uDrT", "udrt", 2),
    ] {
        let (app, ctx, _dir) = app();
        fund(&ctx);
        assert!(submit(&app, signed(alias, 2, 0)).await.is_success());
        let input = ctx.mempool.lock().unwrap().take_snapshot(1).pop().unwrap();
        let dytallix_fast_node::storage::tx::TxMessage::Send {
            denom: actual,
            amount: actual_amount,
            ..
        } = &input.messages.as_ref().unwrap()[0]
        else {
            panic!("send expected")
        };
        assert_eq!(actual, denom);
        assert_eq!(*actual_amount, amount);
        let mut state = ctx.state.lock().unwrap();
        let outcome = dytallix_fast_node::execution::execute_transaction(
            &input,
            &mut state,
            1,
            0,
            &dytallix_fast_node::gas::GasSchedule::default(),
            None,
        )
        .unwrap();
        assert!(outcome.success);
        assert_eq!(state.get_balance("receiver", denom), amount);
        assert_eq!(
            state.get_balance("alice", "udrt"),
            100_000_000 - 21_000_000 - if denom == "udrt" { amount } else { 0 }
        );
    }
}
#[tokio::test]
async fn public_future_nonce_waits_until_the_gap_is_filled() {
    let (app, ctx, _dir) = app();
    fund(&ctx);
    assert!(submit(&app, signed("udgt", 1, 1)).await.is_success());
    assert_eq!(ctx.mempool.lock().unwrap().total_count(), 1);
    assert!(ctx.mempool.lock().unwrap().take_snapshot(10).is_empty());
    assert!(submit(&app, signed("udgt", 1, 0)).await.is_success());
    assert_eq!(
        ctx.mempool
            .lock()
            .unwrap()
            .take_snapshot(10)
            .iter()
            .map(|tx| tx.nonce)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}
#[tokio::test]
async fn public_whole_token_overflow_does_not_enter_the_queue() {
    let (app, ctx, _dir) = app();
    fund(&ctx);
    let before = ctx.state.lock().unwrap().get_balance("alice", "udrt");
    assert_eq!(
        submit(&app, signed("DRT", u128::MAX, 0)).await,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert_eq!(ctx.mempool.lock().unwrap().total_count(), 0);
    assert_eq!(
        ctx.state.lock().unwrap().get_balance("alice", "udrt"),
        before
    );
}

#[tokio::test]
async fn signed_record_preserves_original_alias_through_settlement_and_restart() {
    use dytallix_fast_node::{
        addr::{initial_address, AddressNetwork, OriginKeyAlgorithm},
        crypto::{ActivePQC, PQC},
        types::tx::{Msg, SignedTx, Tx},
    };
    let (sk, pk) = ActivePQC::keypair();
    #[cfg(feature = "pqc-fips204")]
    let algorithm = OriginKeyAlgorithm::MlDsa65;
    #[cfg(not(feature = "pqc-fips204"))]
    let algorithm = OriginKeyAlgorithm::LegacyDilithium5;
    let owner = initial_address(
        AddressNetwork::Development,
        "admission-local",
        algorithm,
        &pk,
    )
    .unwrap();
    let (app, ctx, dir) = app_for_owner(&owner, true);
    let mut admission_config = ctx.mempool.lock().unwrap().config().clone();
    admission_config.min_gas_price = 500;
    *ctx.mempool.lock().unwrap() = Mempool::with_config(admission_config);
    let original = SignedTx::sign(
        Tx {
            chain_id: "admission-local".into(),
            nonce: 0,
            msgs: vec![Msg::Send {
                from: owner,
                to: "receiver".into(),
                denom: "DGT".into(),
                amount: 2,
            }],
            fee: 21_000_000,
            memo: String::new(),
        },
        &sk,
        &pk,
    )
    .unwrap();
    let hash = original.tx_hash().unwrap();
    assert!(submit(&app, original.clone()).await.is_success());
    let input = ctx.mempool.lock().unwrap().take_snapshot(1).pop().unwrap();
    assert_eq!(input.gas_price, 500);
    assert_eq!(input.gas_limit, 42_000);
    // Recovery must use stored conversion inputs, not current queue configuration.
    *ctx.mempool.lock().unwrap() = Mempool::new();
    let before = ctx.storage.get_transaction_record(&hash).unwrap().unwrap();
    let restored: dytallix_fast_node::types::tx::SignedTx = serde_json::from_value(
        serde_json::to_value(before.signed_envelope.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(restored, original);
    restored.verify().unwrap();
    assert_eq!(before.transaction.amount, 2_000_000);
    assert_eq!(before.transaction.denom, "udgt");
    {
        let mut emission = ctx.emission.lock().unwrap();
        let mut staking = ctx.staking.lock().unwrap();
        let mut state = ctx.state.lock().unwrap();
        let mut burn = ctx.fee_burn.lock().unwrap();
        let outcome = dytallix_fast_node::block_settlement::commit_reward_development_block(
            &mut state,
            &mut emission,
            &mut staking,
            &mut burn,
            true,
            &dytallix_fast_node::block_settlement::BlockRequest {
                height: 1,
                transactions: std::slice::from_ref(&input),
                assets: &[],
                timestamp: 10,
                empty_blocks: false,
            },
        )
        .unwrap();
        assert!(outcome.receipts[0].success);
    }
    let settled = ctx.storage.get_transaction_record(&hash).unwrap().unwrap();
    assert_eq!(settled.encode().unwrap(), before.encode().unwrap());
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/transactions/{hash}/record"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 100_000)
        .await
        .unwrap();
    let fetched: dytallix_fast_node::storage::transaction_record::TransactionRecord =
        serde_json::from_slice(&body).unwrap();
    assert_eq!(fetched.encode().unwrap(), before.encode().unwrap());
    drop((app, ctx));
    let reopened = Storage::open(dir.path().join("node.db")).unwrap();
    assert_eq!(
        reopened
            .get_transaction_record(&hash)
            .unwrap()
            .unwrap()
            .encode()
            .unwrap(),
        before.encode().unwrap()
    );
    dytallix_fast_node::block_settlement::verify_recovery(&reopened).unwrap();
    let restored: dytallix_fast_node::types::tx::SignedTx = serde_json::from_value(
        serde_json::to_value(
            reopened
                .get_transaction_record(&hash)
                .unwrap()
                .unwrap()
                .signed_envelope
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    restored.verify().unwrap();
}

#[tokio::test]
async fn transaction_record_route_distinguishes_missing_and_invalid_storage() {
    let (app, ctx, _dir) = app();
    for (hash, status) in [
        ("missing", axum::http::StatusCode::NOT_FOUND),
        ("legacy", axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        if hash == "legacy" {
            ctx.storage.db.put("tx:legacy", b"unsupported").unwrap();
        }
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/transactions/{hash}/record"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn qualified_and_orphan_namespaces_close_legacy_mutation_routes() {
    for marker in ["consensus:orphan", "recovery:orphan", "ordinary:v1:state"] {
        let (read_app, mut ctx, _dir) = app();
        ctx.features.governance = true; // A route flag cannot override account profile isolation.
        ctx.storage.db.put(marker, b"malformed").unwrap();
        let mut routes = Router::new()
            .route("/submit", post(rpc::submit))
            .route("/faucet", post(rpc::faucet))
            .route("/dev/faucet", post(rpc::dev_faucet))
            .route("/asset/register", post(rpc::asset_register))
            .route("/gov/submit", post(rpc::gov_submit_proposal))
            .route("/gov/deposit", post(rpc::gov_deposit))
            .route("/gov/vote", post(rpc::gov_vote))
            .route("/gov/execute", post(rpc::gov_execute))
            .route("/bridge/ingest", post(rpc::bridge_ingest))
            .route("/bridge/halt", post(rpc::bridge_halt))
            .route("/ai/score", post(rpc::ai::ai_score));
        let mut requests = vec![
            ("/submit", json!({"signed_tx":signed("udgt",1,0)})),
            (
                "/faucet",
                json!({"address":"dyt1recipient","dgt_amount":1,"drt_amount":1}),
            ),
            (
                "/dev/faucet",
                json!({"address":"recipient","udgt":1,"udrt":1}),
            ),
            ("/asset/register", json!({"params":["asset-hash","{}"]})),
            (
                "/gov/submit",
                json!({"title":"proposal","description":"description","key":"gas_limit","value":"1"}),
            ),
            (
                "/gov/deposit",
                json!({"depositor":"alice","proposal_id":1,"amount":1}),
            ),
            (
                "/gov/vote",
                json!({"voter":"alice","proposal_id":1,"option":"yes"}),
            ),
            ("/gov/execute", json!({"proposal_id":1})),
            (
                "/bridge/ingest",
                json!({"id":"id","source_chain":"a","dest_chain":"b","asset":"udgt","amount":"1","recipient":"alice","signatures":[],"signers":[]}),
            ),
            ("/bridge/halt", json!({"action":"halt"})),
            (
                "/ai/score",
                json!({"tx":{"hash":"h","from":"alice","to":"bob","amount":1,"fee":1,"nonce":0}}),
            ),
        ];
        #[cfg(feature = "contracts")]
        {
            routes = routes
                .route("/contracts/deploy", post(rpc::contracts_deploy))
                .route("/contracts/call", post(rpc::contracts_call));
            requests.push(("/contracts/deploy", json!({"code":"00","deployer":"alice"})));
            requests.push((
                "/contracts/call",
                json!({"address":"contract","method":"mutate"}),
            ));
        }
        #[cfg(feature = "oracle")]
        {
            routes = routes
                .route("/oracle/risk", post(rpc::oracle::submit_ai_risk))
                .route("/oracle/batch", post(rpc::oracle::submit_ai_risk_batch));
            requests.push((
                "/oracle/risk",
                json!({"tx_hash":"h","model_id":"m","risk_score":0.2}),
            ));
            requests.push(("/oracle/batch", json!({"records":[]})));
        }
        let routes = routes.layer(Extension(ctx.clone()));
        let database = || {
            ctx.storage
                .db
                .iterator(rocksdb::IteratorMode::Start)
                .map(|entry| entry.unwrap())
                .collect::<Vec<_>>()
        };
        let before = database();
        let accounts = serde_json::to_value(&ctx.state.lock().unwrap().accounts).unwrap();
        for (route, body) in requests {
            let response = routes
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri(route)
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::BAD_REQUEST,
                "{marker}: {route}"
            );
            let bytes = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert!(
                String::from_utf8_lossy(&bytes).contains("requires consensus admission"),
                "{marker}: {route}"
            );
            assert_eq!(database(), before, "{marker}: {route}");
            assert_eq!(
                serde_json::to_value(&ctx.state.lock().unwrap().accounts).unwrap(),
                accounts
            );
            assert_eq!(ctx.mempool.lock().unwrap().total_count(), 0);
            assert!(ctx.pending_assets.lock().unwrap().is_empty());
        }
        // Read access remains available; corrupt unrelated profile bytes do not authorize writes.
        let response = read_app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/balance/alice")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
}

#[tokio::test]
async fn qualified_store_rejects_legacy_mempool_entry_and_reconciliation_without_changes() {
    for marker in ["consensus:orphan", "recovery:orphan", "ordinary:orphan"] {
        let (app, ctx, _dir) = app();
        assert!(submit(&app, signed("udgt", 1, 0)).await.is_success());
        let input = ctx.mempool.lock().unwrap().take_snapshot(1).pop().unwrap();
        ctx.storage.db.put(marker, b"malformed").unwrap();
        let state = ctx.state.lock().unwrap();
        let mut queue = ctx.mempool.lock().unwrap();
        let snapshot = serde_json::to_value(queue.take_snapshot(100)).unwrap();
        let bytes = queue.total_bytes();
        for trusted in [false, true] {
            let result = if trusted {
                queue.add_transaction_trusted(&state, input.clone())
            } else {
                queue.add_transaction(&state, input.clone())
            };
            assert!(matches!(
                result,
                Err(dytallix_fast_node::mempool::RejectionReason::PolicyViolation(_))
            ));
            assert_eq!(
                serde_json::to_value(queue.take_snapshot(100)).unwrap(),
                snapshot
            );
            assert_eq!(queue.total_bytes(), bytes);
        }
        assert!(queue
            .reconcile(&state, std::slice::from_ref(&input.hash))
            .is_err());
        assert_eq!(
            serde_json::to_value(queue.take_snapshot(100)).unwrap(),
            snapshot
        );
        assert!(dytallix_fast_node::mempool::basic_validate(&state, &input).is_err());
    }
}

#[test]
fn legacy_genesis_cannot_initialize_or_reopen_qualified_or_orphan_state() {
    for initialized in [false, true] {
        for marker in ["consensus:orphan", "recovery:orphan", "ordinary:v1:state"] {
            let directory = tempdir().unwrap();
            let mut storage = Storage::open(directory.path().join("db")).unwrap();
            let source=br#"{"chain_id":"guard-local","accounts":[{"address":"alice","balances":{"udgt":"100"}}]}"#;
            if initialized {
                dytallix_fast_node::genesis::initialize(&mut storage, "guard-local", Some(source))
                    .unwrap();
            }
            storage.db.put(marker, b"malformed").unwrap();
            let before = storage
                .db
                .iterator(rocksdb::IteratorMode::Start)
                .map(|entry| entry.unwrap())
                .collect::<Vec<_>>();
            let error =
                dytallix_fast_node::genesis::initialize(&mut storage, "guard-local", Some(source))
                    .unwrap_err();
            assert!(error.to_string().contains("requires consensus admission"));
            let after = storage
                .db
                .iterator(rocksdb::IteratorMode::Start)
                .map(|entry| entry.unwrap())
                .collect::<Vec<_>>();
            assert_eq!(after, before, "{initialized}: {marker}");
        }
    }
}

#[cfg(feature = "pqc-fips204")]
#[tokio::test]
async fn receipt_algorithm_uses_original_signature_and_omits_unknown_metadata() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
    use fips204::{
        ml_dsa_87,
        traits::{KeyGen, SerDes, Signer},
    };
    let (app, ctx, _dir) = app();
    fund(&ctx);
    assert_eq!(ActivePQC::ALG, "mldsa65");
    let mut original = signed("udgt", 1, 0);
    let (public, secret) = ml_dsa_87::KG::keygen_from_seed(&[71; 32]);
    original.algorithm = "mldsa87".into();
    original.public_key = B64.encode(public.into_bytes());
    original.signature = B64.encode(
        secret
            .try_sign(&sha3_256(&canonical_json(&original.tx).unwrap()), &[])
            .unwrap(),
    );
    original.verify().unwrap();
    let hash = original.tx_hash().unwrap();
    assert!(submit(&app, original).await.is_success());
    let response =
        rpc::get_transaction_receipt(axum::extract::Path(hash.clone()), Extension(ctx.clone()))
            .await
            .unwrap();
    assert_eq!(response.0["pqc_algorithm"], "mldsa87");
    // Old receipts can lack an original signed record. Do not infer an algorithm.
    ctx.storage.db.delete(format!("tx:{hash}")).unwrap();
    let response = rpc::get_transaction_receipt(axum::extract::Path(hash), Extension(ctx))
        .await
        .unwrap();
    assert!(response.0.get("pqc_algorithm").is_none());
}
