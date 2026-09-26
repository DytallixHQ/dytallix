use dytallix_protocol_types::{ordinary, ordinary_client::AccountDomain, ordinary_fees};
use dytallix_sdk::{ordinary_v2::*, DytallixKeypair};
use serde_json::Value;
use std::sync::OnceLock;
fn key() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    KEY.get_or_init(DytallixKeypair::generate)
}
fn profile() -> FeeProfile {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    serde_json::from_value(v["profile"].clone()).unwrap()
}
fn context(p: &FeeProfile, k: &DytallixKeypair) -> SigningContext {
    let algorithm = match k.scheme() {
        dytallix_sdk::KeyScheme::MlDsa65 => "mldsa65",
        dytallix_sdk::KeyScheme::MlDsa87 => "mldsa87",
        _ => panic!("test requires ML-DSA"),
    };
    let origin = if algorithm == "mldsa65" {
        OriginKeyAlgorithm::MlDsa65
    } else {
        OriginKeyAlgorithm::MlDsa87
    };
    let a = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "sdk-ordinary",
        origin,
        k.public_key(),
    )
    .unwrap();
    SigningContext {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "sdk-ordinary".into(),
            genesis_digest: [7; 32],
            account_id: *a.account_id(),
        },
        current_key: KeyIdentity {
            algorithm: algorithm.into(),
            public_key: k.public_key().to_vec(),
        },
        authorization_generation: 4,
        spending_nonce: 9,
        committed: CommittedContext {
            chain_id: "sdk-ordinary".into(),
            genesis_digest: [7; 32],
            height: 19,
            app_hash: [8; 32],
        },
        profile_digest: profile_digest(p).unwrap(),
        protected: false,
    }
}
fn prepared(p: &FeeProfile, c: &SigningContext) -> PreparedTransaction {
    prepare(
        p,
        c,
        vec![Action::Data {
            data: "client fixture".into(),
        }],
        "memo".into(),
        25,
        1000,
        3000,
    )
    .unwrap()
}
fn signed() -> (FeeProfile, SigningContext, SignedOrdinary) {
    let mut p = profile();
    p.wire_byte_cost = 0;
    let c = context(&p, key());
    let s = prepared(&p, &c)
        .sign(&KeypairSigner::new(key()).unwrap())
        .unwrap();
    (p, c, s)
}
fn receipt(p: &FeeProfile, c: &SigningContext, s: &SignedOrdinary) -> ReceiptView {
    ReceiptView {
        version: 1,
        context: CommittedContext {
            height: 20,
            ..c.committed.clone()
        },
        transaction_id: transaction_id(&s.body, &p.limits).unwrap(),
        envelope_hash: envelope_hash(s, &p.limits).unwrap(),
        actor: s.body.domain.account_id,
        block_height: 20,
        block_index: 0,
        contract_version: 1,
        profile_version: p.version,
        profile_digest: profile_digest(p).unwrap(),
        outcome: ReceiptOutcome::Success,
        failing_action: None,
        failure_phase: None,
        rule_class: None,
        rule_code: None,
        gas_limit: 1000,
        gas_used: 400,
        metadata_gas: p.receipt_metadata_cost,
        reserved_cap: 3000,
        charge: 800,
        released_cap: 2200,
        nonce_before: 9,
        nonce_after: 10,
    }
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}
#[test]
fn shared_all_action_and_minimal_golden_vectors_match_exact_node_bytes() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v2_vectors.json"
    ))
    .unwrap();
    let limits: Limits = serde_json::from_value(v["limits"].clone()).unwrap();
    for vector in v["vectors"].as_array().unwrap() {
        let signed: SignedOrdinary = serde_json::from_value(vector["input"].clone()).unwrap();
        assert_eq!(
            hex(&ordinary::signing_bytes(&signed.body, &limits).unwrap()),
            vector["expected_signing_bytes_hex"].as_str().unwrap()
        );
        assert_eq!(
            hex(&ordinary::encode(&signed, &limits).unwrap()),
            vector["expected_envelope_hex"].as_str().unwrap()
        );
        assert_eq!(
            hex(&transaction_id(&signed.body, &limits).unwrap()),
            vector["expected_transaction_id"].as_str().unwrap()
        );
        assert_eq!(
            hex(&envelope_hash(&signed, &limits).unwrap()),
            vector["expected_envelope_hash"].as_str().unwrap()
        );
        assert!(verify_signature(&signed, &limits).is_err());
    }
}
#[test]
fn six_place_amounts_are_exact_and_overflow_is_rejected() {
    for (text, n) in [
        ("0", 0),
        ("1", 1_000_000),
        ("0.000001", 1),
        ("123.456789", 123456789),
        ("1.000000", 1_000_000),
    ] {
        assert_eq!(parse_token_units(text).unwrap(), n);
    }
    for text in [
        "",
        " 1",
        "+1",
        "-1",
        "01",
        "1.",
        ".1",
        "1.0000001",
        "1e2",
        "1.1.1",
        "340282366920938463463374607431769",
    ] {
        assert!(parse_token_units(text).is_err(), "{text}");
    }
    assert_eq!(
        parse_token_units("340282366920938463463374607431768.211455").unwrap(),
        u128::MAX
    );
    assert!(parse_token_units("340282366920938463463374607431768.211456").is_err());
}
#[test]
fn selected_schemes_sign_complete_canonical_bytes_and_reject_tamper() {
    #[cfg(feature = "compatibility")]
    let secondary = DytallixKeypair::generate_mldsa87();
    #[cfg(not(feature = "compatibility"))]
    let secondary = DytallixKeypair::generate();
    for k in [key(), &secondary] {
        let p = profile();
        let c = context(&p, k);
        let prepared = prepared(&p, &c);
        let signed = prepared.sign(&KeypairSigner::new(k).unwrap()).unwrap();
        verify_signature(&signed, &p.limits).unwrap();
        assert!(prepared
            .sign(&KeypairSigner::new(&DytallixKeypair::generate()).unwrap())
            .is_err());
        let mut tampered = signed.clone();
        tampered.body.maximum_fee += 1;
        assert!(verify_signature(&tampered, &p.limits).is_err());
        let mut tampered = signed;
        tampered.signature[0] ^= 1;
        assert!(verify_signature(&tampered, &p.limits).is_err());
    }
}
#[test]
fn profile_authority_and_next_height_expiry_are_checked_before_signing() {
    let p = profile();
    let c = context(&p, key());
    let b = prepared(&p, &c).body().clone();
    let mut mutations = Vec::new();
    let mut v = b.clone();
    v.domain.genesis_digest[0] ^= 1;
    mutations.push(v);
    let mut v = b.clone();
    v.domain.account_id[0] ^= 1;
    mutations.push(v);
    let mut v = b.clone();
    v.domain.chain_id.push('x');
    mutations.push(v);
    let mut v = b.clone();
    v.authorization_generation += 1;
    mutations.push(v);
    let mut v = b.clone();
    v.spending_nonce += 1;
    mutations.push(v);
    let mut v = b.clone();
    v.key.public_key[0] ^= 1;
    mutations.push(v);
    let mut v = b.clone();
    v.fee_profile_digest[0] ^= 1;
    mutations.push(v);
    let mut v = b.clone();
    v.maximum_fee = 1999;
    mutations.push(v);
    let mut v = b.clone();
    v.expiry_height = 20;
    mutations.push(v);
    let mut v = b.clone();
    v.expiry_height = 1021;
    mutations.push(v);
    for v in mutations {
        assert!(PreparedTransaction::from_body(v, &p, &c).is_err());
    }
    let mut locked = c.clone();
    locked.protected = true;
    assert!(PreparedTransaction::from_body(b.clone(), &p, &locked).is_err());
    let mut exhausted = c.clone();
    exhausted.committed.height = u64::MAX;
    assert!(PreparedTransaction::from_body(b, &p, &exhausted).is_err());
    let mut genesis_p = p;
    genesis_p.activation_height = 1;
    let mut genesis_c = context(&genesis_p, key());
    genesis_c.committed.height = 0;
    assert!(prepare(
        &genesis_p,
        &genesis_c,
        vec![Action::DmsPing],
        String::new(),
        2,
        1000,
        3000
    )
    .is_ok());
    assert!(prepare(
        &genesis_p,
        &genesis_c,
        vec![Action::DmsPing],
        String::new(),
        1,
        1000,
        3000
    )
    .is_err());
}
#[test]
fn cap_quote_is_separate_from_charge_and_does_not_invent_defaults() {
    let p = profile();
    let q = FeeQuote::from_profile(&p, 1000).unwrap();
    assert_eq!(q.required_cap, 2000);
    assert_eq!(q.minimum_charge, 20);
    assert!(FeeQuote::from_profile(&p, 0).is_err());
    assert!(FeeQuote::from_profile(&p, 100001).is_err());
    let mut p = p;
    p.gas_price = 0;
    assert!(FeeQuote::from_profile(&p, 1000).is_err());
}
#[test]
fn strict_transport_preserves_binary_and_rejects_ambiguous_views() {
    let (p, _, s) = signed();
    let raw = encode_transport(&s, &p.limits, 100_000).unwrap();
    assert_eq!(decode_transport(&raw, &p.limits, raw.len()).unwrap(), s);
    assert!(decode_transport(&raw, &p.limits, raw.len() - 1).is_err());
    let text = String::from_utf8(raw.clone()).unwrap();
    for suffix in [",\"type\":\"ordinary_v2\"}", ",\"extra\":0}"] {
        let bad = format!("{}{}", &text[..text.len() - 1], suffix);
        assert!(decode_transport(bad.as_bytes(), &p.limits, 100_000).is_err());
    }
    let mut value: Value = serde_json::from_slice(&raw).unwrap();
    let encoded = value["envelope_base64"].as_str().unwrap().to_string();
    value["envelope_base64"] = Value::String(format!(" {encoded}"));
    assert!(decode_transport(&serde_json::to_vec(&value).unwrap(), &p.limits, 100_000).is_err());
}
#[test]
fn receipt_checks_bind_identity_charge_release_nonce_and_failure_rules() {
    let (p, c, s) = signed();
    let r = receipt(&p, &c, &s);
    validate_receipt(&r, &s, &p).unwrap();
    let mut bads = Vec::new();
    let mut v = r.clone();
    v.transaction_id[0] ^= 1;
    bads.push(v);
    let mut v = r.clone();
    v.envelope_hash[0] ^= 1;
    bads.push(v);
    let mut v = r.clone();
    v.profile_digest[0] ^= 1;
    bads.push(v);
    let mut v = r.clone();
    v.actor[0] ^= 1;
    bads.push(v);
    let mut v = r.clone();
    v.charge += 1;
    bads.push(v);
    let mut v = r.clone();
    v.released_cap -= 1;
    bads.push(v);
    let mut v = r.clone();
    v.nonce_after += 1;
    bads.push(v);
    let mut v = r.clone();
    v.block_height = s.body.expiry_height;
    v.context.height = v.block_height;
    bads.push(v);
    let mut v = r.clone();
    v.failing_action = Some(0);
    bads.push(v);
    for v in bads {
        assert!(validate_receipt(&v, &s, &p).is_err());
    }
    let mut oog = r;
    oog.outcome = ReceiptOutcome::OutOfGas;
    oog.gas_used = 1000;
    oog.charge = 2000;
    oog.released_cap = 1000;
    oog.failing_action = Some(0);
    oog.failure_phase = Some("WRITE".into());
    oog.rule_code = Some("OUT_OF_GAS".into());
    validate_receipt(&oog, &s, &p).unwrap();
    oog.rule_class = Some("unknown".into());
    assert!(validate_receipt(&oog, &s, &p).is_err());
    oog.rule_class = None;
    oog.failing_action = None;
    assert!(validate_receipt(&oog, &s, &p).is_err());
}
#[test]
fn context_views_require_exact_height_hash_identity_and_current_authority() {
    let p = profile();
    let c = context(&p, key());
    let pv = ProfileView {
        version: 1,
        enabled: true,
        context: c.committed.clone(),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: p.clone(),
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 10,
            max_transport_bytes: 200_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    };
    let av = AccountView {
        version: 1,
        context: c.committed.clone(),
        domain: AccountDomain {
            network: 3,
            chain_id: c.domain.chain_id.clone(),
            genesis_digest: c.domain.genesis_digest,
            account_id: c.domain.account_id,
        },
        account_id: c.domain.account_id,
        address: AccountAddress::from_account_id(AddressNetwork::Development, c.domain.account_id)
            .encode(),
        current_key: c.current_key.clone(),
        authorization_generation: c.authorization_generation,
        spending_nonce: c.spending_nonce,
        protected: false,
        profile_digest: c.profile_digest,
    };
    assert_eq!(validate_views(&c, &pv, &av).unwrap(), p);
    let mut bad = av.clone();
    bad.context.height += 1;
    assert!(validate_views(&c, &pv, &bad).is_err());
    let mut bad = av.clone();
    bad.context.app_hash[0] ^= 1;
    assert!(validate_views(&c, &pv, &bad).is_err());
    let mut bad = av.clone();
    bad.spending_nonce += 1;
    assert!(validate_views(&c, &pv, &bad).is_err());
    let mut bad = av;
    bad.address =
        AccountAddress::from_account_id(AddressNetwork::Testnet, c.domain.account_id).encode();
    assert!(validate_views(&c, &pv, &bad).is_err());
}
#[test]
fn full_width_receipt_values_remain_canonical_decimal_strings() {
    let (p, c, s) = signed();
    let mut r = receipt(&p, &c, &s);
    r.reserved_cap = u128::MAX;
    r.released_cap = u128::MAX - r.charge;
    r.context.height = u64::MAX;
    let raw = serde_json::to_string(&r).unwrap();
    assert!(raw.contains(&format!("\"{}\"", u128::MAX)));
    assert_eq!(serde_json::from_str::<ReceiptView>(&raw).unwrap(), r);
    assert!(serde_json::from_str::<ReceiptView>(
        &raw.replace(&format!("\"{}\"", u128::MAX), &u128::MAX.to_string())
    )
    .is_err());
}
#[test]
fn shared_profile_vector_digest_is_preserved() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    assert_eq!(
        hex(&ordinary_fees::profile_digest(&profile()).unwrap()),
        v["expected_digest"].as_str().unwrap()
    );
}

#[cfg(any(
    feature = "network",
    feature = "ordinary-http-only",
    feature = "strict-local-mldsa65"
))]
mod rpc_tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use dytallix_sdk::ordinary_client::CometClient;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn server(reply: String) -> (String, thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut raw = Vec::new();
            let mut buf = [0; 4096];
            let (header_len, body_len) = loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8(raw[..end].to_vec()).unwrap();
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, len);
                }
            };
            while raw.len() < header_len + body_len {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
            }
            let request = serde_json::from_slice(&raw[header_len..header_len + body_len]).unwrap();
            let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",reply.len());
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(reply.as_bytes());
            request
        });
        (url, task)
    }
    fn envelope(result: Value) -> String {
        serde_json::json!({"jsonrpc":"2.0","id":"ordinary-v2","result":result}).to_string()
    }
    #[tokio::test]
    async fn rpc_submission_binds_exact_transport_hash_and_reports_only_checktx() {
        use sha2::{Digest, Sha256};
        let (p, _, s) = signed();
        let raw = encode_transport(&s, &p.limits, 100_000).unwrap();
        let expected = hex(&Sha256::digest(&raw));
        for (hash, valid) in [(expected.to_uppercase(), true), ("00".repeat(32), false)] {
            let (url, task) = server(envelope(
                serde_json::json!({"code":0,"log":"","codespace":"","hash":hash,"data":""}),
            ));
            let result = CometClient::new(&url, 100_000)
                .unwrap()
                .submit_sync(&s, &p, 100_000)
                .await;
            if valid {
                let result = result.unwrap();
                assert!(result.admitted());
                assert!(result.submitted);
            } else {
                assert!(result.is_err());
            }
            let request = task.join().unwrap();
            assert_eq!(request["method"], "broadcast_tx_sync");
            assert_eq!(
                STANDARD
                    .decode(request["params"]["tx"].as_str().unwrap())
                    .unwrap(),
                raw
            );
        }
    }
    #[tokio::test]
    async fn rpc_context_height_bounds_and_response_identity_are_strict() {
        let (p, c, s) = signed();
        let view = ProfileView {
            version: 1,
            enabled: false,
            context: c.committed.clone(),
            config: None,
        };
        let (url, task) = server(envelope(
            serde_json::json!({"response":{"code":0,"log":"","height":"19","value":STANDARD.encode(serde_json::to_vec(&view).unwrap())}}),
        ));
        assert_eq!(
            CometClient::new(&url, 100_000)
                .unwrap()
                .query_profile()
                .await
                .unwrap(),
            view
        );
        let request = task.join().unwrap();
        assert_eq!(request["method"], "abci_query");
        assert_eq!(request["params"]["path"], "/ordinary/profile");
        assert_eq!(request["params"]["prove"], false);
        for reply in [
            "{\"jsonrpc\":\"2.0\",\"id\":\"wrong\",\"result\":{\"code\":0,\"log\":\"\",\"codespace\":\"\"}}".to_string(),
            "{\"jsonrpc\":\"2.0\",\"id\":\"ordinary-v2\",\"id\":\"ordinary-v2\",\"result\":{\"code\":0,\"log\":\"\",\"codespace\":\"\"}}".to_string(),
            "{\"jsonrpc\":\"2.0\",\"id\":\"ordinary-v2\",\"result\":{\"code\":0,\"log\":\"\",\"codespace\":\"\"},\"error\":{\"code\":-1,\"message\":\"bad\"}}".to_string(),
            envelope(serde_json::json!({"code":0,"log":"","codespace":"","extra":true})),
            envelope(serde_json::json!({"log":"","codespace":""})),
        ] {
            let (url,task)=server(reply);assert!(CometClient::new(&url,100_000).unwrap().check_tx(&s,&p,100_000).await.is_err());task.join().unwrap();
        }
        let (url, task) = server("x".repeat(1001));
        assert!(CometClient::new(&url, 1000)
            .unwrap()
            .query_profile()
            .await
            .is_err());
        task.join().unwrap();
    }
    #[tokio::test]
    async fn missing_committed_receipt_is_distinct_from_checktx_admission() {
        let (p, _, s) = signed();
        let id = transaction_id(&s.body, &p.limits).unwrap();
        let (url, task) = server(envelope(
            serde_json::json!({"response":{"code":0,"log":"","height":"19","value":STANDARD.encode(b"null")}}),
        ));
        assert!(CometClient::new(&url, 100_000)
            .unwrap()
            .query_receipt(&id)
            .await
            .unwrap()
            .is_none());
        task.join().unwrap();
        let (url, task) = server(envelope(
            serde_json::json!({"code":0,"log":"","codespace":"","gas_wanted":"1000","gas_used":"0","events":[]}),
        ));
        let response = CometClient::new(&url, 100_000)
            .unwrap()
            .check_tx(&s, &p, 100_000)
            .await
            .unwrap();
        assert!(response.admitted());
        assert!(!response.submitted);
        assert!(response.engine_hash.is_none());
        task.join().unwrap();
    }
}

#[test]
fn receipt_application_failure_zero_height_and_validation_gas_floor() {
    let (mut p, _, _) = signed();
    p.activation_height = 0;
    let c = context(&p, key());
    let s = prepare(
        &p,
        &c,
        vec![Action::DmsClaim {
            owner: [3; 32],
            expected_grant_generation: 0,
        }],
        String::new(),
        25,
        1000,
        3000,
    )
    .unwrap()
    .sign(&KeypairSigner::new(key()).unwrap())
    .unwrap();
    let mut r = receipt(&p, &c, &s);
    r.outcome = ReceiptOutcome::ApplicationFailure;
    r.failing_action = Some(0);
    r.failure_phase = Some("APPLICATION".into());
    r.rule_class = Some("ACTION_STATE_PRECONDITION".into());
    r.rule_code = Some("DMS_INACTIVITY_DELAY".into());
    validate_receipt(&r, &s, &p).unwrap();
    let mut wrong = r.clone();
    wrong.rule_class = Some("ACTION_CAPACITY".into());
    assert!(validate_receipt(&wrong, &s, &p).is_err());
    let mut wrong = r.clone();
    wrong.block_height = 0;
    assert!(validate_receipt(&wrong, &s, &p).is_err());
    // Preserve exact charge and release conservation while omitting known
    // validation work. The mandatory gas floor must reject this receipt.
    let mut wrong = r;
    wrong.gas_used = 1;
    wrong.charge = u128::from(p.minimum_gas) * u128::from(p.gas_price);
    wrong.released_cap = wrong.reserved_cap - wrong.charge;
    assert!(validate_receipt(&wrong, &s, &p).is_err());
}
