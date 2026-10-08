use super::*;
use crate::consensus_settlement::ConsensusConfig;
use std::path::PathBuf;
use std::sync::Mutex;

/// The rehearsal configuration this build accepts: all three root policies
/// at schema 2.
fn config() -> ConsensusConfig {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures")
        .join(if crate::build_profile::PRODUCTION {
            "genesis-production-rehearsal"
        } else {
            "genesis-rehearsal"
        })
        .join("application-config.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn policies(config: &ConsensusConfig) -> Policies<'_> {
    Policies::of(
        config.emergency.as_ref(),
        config.upgrade.as_ref(),
        config.release_handover.as_ref(),
    )
    .unwrap()
}

/// The only signature the test helper accepts from a key over an artifact.
fn signed(public_key_hex: &str, artifact: &[u8]) -> String {
    let seed = Sha256::digest([&hex::decode(public_key_hex).unwrap(), artifact].concat());
    B64.encode(
        seed.iter()
            .cycle()
            .take(SIGNATURE_BYTES)
            .copied()
            .collect::<Vec<_>>(),
    )
}
/// A request the helper saw: key, sequence, height and window.
type Call = (String, u64, u64, u64, u64);
#[derive(Default)]
struct Helper {
    calls: Mutex<Vec<Call>>,
    unavailable: bool,
}
impl upgrade::Verifier for Helper {
    fn verify(
        &self,
        chain_id: &str,
        public_key: &[u8],
        sequence: u64,
        current_height: u64,
        not_before_height: u64,
        not_after_height: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        assert_eq!(
            chain_id,
            config().upgrade.unwrap().as_v2().unwrap().chain_id
        );
        assert!(artifact.starts_with(DOMAIN));
        self.calls.lock().unwrap().push((
            hex::encode(public_key),
            sequence,
            current_height,
            not_before_height,
            not_after_height,
        ));
        anyhow::ensure!(!self.unavailable, "helper unavailable");
        Ok(B64.encode(signature) == signed(&hex::encode(public_key), artifact))
    }
}

fn new_key(seed: u8) -> AuthorityKey {
    let public = [seed; KEY_BYTES];
    AuthorityKey {
        key_id: hex::encode(Sha256::digest(public)),
        public_key_hex: hex::encode(public),
    }
}
fn context(height: u64) -> BlockContext {
    BlockContext {
        height,
        finalized_anchor: Some(FinalizedAnchor {
            height: height - 1,
            app_hash: "ab".repeat(32),
        }),
    }
}

/// A replacement of each role's first key at `height`, signed by three
/// upgrade keys of the epoch in force, with the new keys' proofs.
fn control(policies: &Policies, record: &Record, height: u64, seed: u8) -> Control {
    let epoch = record.at(height).unwrap();
    let anchor = context(height).finalized_anchor.unwrap();
    let mut control = Control {
        kind: CONTROL_KIND.into(),
        payload: Payload {
            schema: 1,
            chain_id: policies.upgrade_v2.chain_id.clone(),
            genesis_sha256: policies.upgrade_v2.genesis_sha256.clone(),
            policy_sha256: policies.upgrade_v2.sha256().unwrap(),
            authority_epoch: epoch.authority_epoch,
            sequence: epoch.authority_epoch + 1,
            anchor_height: anchor.height,
            anchor_app_hash: anchor.app_hash,
            not_before_height: height,
            not_after_height: height + 2,
            replaced: Roles {
                upgrade: epoch.upgrade.keys[0].key_id.clone(),
                freeze: epoch.freeze.keys[0].key_id.clone(),
                resume: epoch.resume.keys[0].key_id.clone(),
            },
            keys: Roles {
                upgrade: new_key(seed),
                freeze: new_key(seed + 1),
                resume: new_key(seed + 2),
            },
        },
        signatures: Vec::new(),
        proofs: Roles {
            upgrade: String::new(),
            freeze: String::new(),
            resume: String::new(),
        },
    };
    sign(&mut control, &epoch.upgrade.keys[1..4]);
    control
}
/// Sign the payload as it now is with `signers` and the new keys.
fn sign(control: &mut Control, signers: &[AuthorityKey]) {
    let artifact = artifact_bytes(&control.payload).unwrap();
    control.signatures = signers
        .iter()
        .map(|key| Signature {
            key_id: key.key_id.clone(),
            signature_base64: signed(&key.public_key_hex, &artifact),
        })
        .collect();
    let keys = &control.payload.keys;
    control.proofs = Roles {
        upgrade: signed(&keys.upgrade.public_key_hex, &artifact),
        freeze: signed(&keys.freeze.public_key_hex, &artifact),
        resume: signed(&keys.resume.public_key_hex, &artifact),
    };
}
fn plan(
    policies: &Policies,
    record: Option<&Record>,
    height: u64,
    control: &Control,
    helper: &Helper,
) -> Result<BlockPlan> {
    plan_block(
        policies,
        record,
        &context(height),
        &serde_json::to_vec(control).unwrap(),
        Some(helper),
    )
}

#[test]
fn a_replacement_brings_the_next_epoch_in_after_the_notice() {
    let config = config();
    let policies = policies(&config);
    let configured = policies.current(None);
    let control = control(&policies, &configured, 10, 0xa0);
    let helper = Helper::default();
    let planned = plan(&policies, None, 10, &control, &helper).unwrap();
    // Three upgrade keys sign and the three new keys prove possession, each
    // for the epoch the replacement creates over its window.
    let calls = helper.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 6);
    assert!(calls
        .iter()
        .all(|call| call.1 == 2 && call.2 == 10 && (call.3, call.4) == (10, 12)));
    let mut signers: Vec<_> = calls.iter().map(|call| call.0.clone()).collect();
    signers.dedup();
    assert_eq!(signers.len(), 6);

    let notice = config
        .release_handover
        .as_ref()
        .unwrap()
        .v2
        .as_ref()
        .unwrap()
        .min_notice_blocks;
    let record = &planned.record;
    assert_eq!(record.epochs.len(), 2);
    assert_eq!(record.epochs[0], policies.configured_epoch());
    let next = &record.epochs[1];
    assert_eq!((next.authority_epoch, next.from_height), (2, 10 + notice));
    assert_eq!(planned.receipt.effect_height, 10 + notice);
    for (old, new, replaced, key) in [
        (
            &configured.epochs[0].upgrade,
            &next.upgrade,
            &control.payload.replaced.upgrade,
            &control.payload.keys.upgrade,
        ),
        (
            &configured.epochs[0].freeze,
            &next.freeze,
            &control.payload.replaced.freeze,
            &control.payload.keys.freeze,
        ),
        (
            &configured.epochs[0].resume,
            &next.resume,
            &control.payload.replaced.resume,
            &control.payload.keys.resume,
        ),
    ] {
        // One key leaves and one arrives; the rest and the threshold stay.
        assert!(new.keys.contains(key) && !new.keys.iter().any(|k| &k.key_id == replaced));
        let kept: Vec<_> = old.keys.iter().filter(|k| &k.key_id != replaced).collect();
        assert!(kept.iter().all(|k| new.keys.contains(k)));
        assert_eq!(
            (new.keys.len(), new.threshold),
            (old.keys.len(), old.threshold)
        );
        assert!(new
            .keys
            .windows(2)
            .all(|pair| pair[0].key_id < pair[1].key_id));
    }
    // The old keys sign until the effect height, the new ones from it.
    assert_eq!(record.at(10 + notice - 1).unwrap().authority_epoch, 1);
    assert_eq!(record.at(10 + notice).unwrap().authority_epoch, 2);
    let emergency = root_authority::emergency_policy(policies.emergency, Some(next)).unwrap();
    assert!(emergency
        .freeze_authority
        .keys
        .contains(&control.payload.keys.freeze));

    // A replay gives the same receipt and record, with or without the helper.
    let receipt = &planned.receipt;
    assert_eq!(
        replay(&policies, None, receipt, &context(10), Some(&helper)).unwrap(),
        planned
    );
    assert_eq!(
        replay(&policies, None, receipt, &context(10), None).unwrap(),
        planned
    );
    assert!(replay(&policies, None, receipt, &context(11), None).is_err());
    let mut changed = receipt.clone();
    changed.effect_height += 1;
    assert!(replay(&policies, None, &changed, &context(10), None).is_err());
    let bytes = encode_receipt(receipt).unwrap();
    assert_eq!(&decode_receipt(&policies, &bytes).unwrap(), receipt);
    let pretty = serde_json::to_vec_pretty(receipt).unwrap();
    assert!(decode_receipt(&policies, &pretty).is_err());
}

#[test]
fn admission_refuses_every_broken_binding_before_any_signature_work() {
    let config = config();
    let policies = policies(&config);
    let record = policies.current(None);
    let good = control(&policies, &record, 10, 0xa0);
    let upgrade_keys = record.epochs[0].upgrade.keys.clone();
    let freeze_key = record.epochs[0].freeze.keys[1].clone();
    let validity = policies.upgrade_v2.max_validity_blocks;
    type Change = Box<dyn Fn(&mut Control)>;
    let unsigned: Vec<(&str, Change)> = vec![
        ("schema", Box::new(|c| c.payload.schema = 2)),
        ("chain", Box::new(|c| c.payload.chain_id.push('x'))),
        (
            "genesis",
            Box::new(|c| c.payload.genesis_sha256 = "00".repeat(32)),
        ),
        (
            "policy",
            Box::new(|c| c.payload.policy_sha256 = "00".repeat(32)),
        ),
        ("old epoch", Box::new(|c| c.payload.authority_epoch = 0)),
        ("next epoch", Box::new(|c| c.payload.authority_epoch = 2)),
        ("sequence", Box::new(|c| c.payload.sequence = 3)),
        (
            "window start",
            Box::new(|c| c.payload.not_before_height = 11),
        ),
        ("window end", Box::new(|c| c.payload.not_after_height = 9)),
        (
            "window length",
            Box::new(move |c| c.payload.not_after_height = 10 + validity),
        ),
        ("anchor height", Box::new(|c| c.payload.anchor_height = 8)),
        (
            "anchor hash",
            Box::new(|c| c.payload.anchor_app_hash = "cd".repeat(32)),
        ),
        (
            "replaced upgrade",
            Box::new(|c| c.payload.replaced.upgrade = c.payload.replaced.freeze.clone()),
        ),
        (
            "replaced freeze",
            Box::new(|c| c.payload.replaced.freeze = "00".repeat(32)),
        ),
        (
            "replaced resume",
            Box::new(|c| c.payload.replaced.resume = c.payload.replaced.upgrade.clone()),
        ),
        (
            "key id",
            Box::new(|c| c.payload.keys.freeze.key_id = "00".repeat(32)),
        ),
        (
            "key size",
            Box::new(|c| c.payload.keys.resume.public_key_hex.push_str("00")),
        ),
        (
            "repeated new key",
            Box::new(|c| c.payload.keys.resume = c.payload.keys.freeze.clone()),
        ),
        (
            "held key",
            Box::new(move |c| c.payload.keys.freeze = freeze_key.clone()),
        ),
        (
            "held upgrade key",
            Box::new(move |c| c.payload.keys.upgrade = upgrade_keys[3].clone()),
        ),
    ];
    let helper = Helper::default();
    for (name, change) in unsigned {
        let mut control = good.clone();
        change(&mut control);
        let signers = record.epochs[0].upgrade.keys[1..4].to_vec();
        sign(&mut control, &signers);
        assert!(
            plan(&policies, None, 10, &control, &helper).is_err(),
            "{name}"
        );
    }
    // Signature sets and encodings.
    let epoch = &record.epochs[0];
    let mut signer_sets: Vec<(&str, Vec<AuthorityKey>)> = vec![
        ("two signers", epoch.upgrade.keys[1..3].to_vec()),
        (
            "freeze signer",
            vec![
                epoch.upgrade.keys[1].clone(),
                epoch.upgrade.keys[2].clone(),
                epoch.freeze.keys[0].clone(),
            ],
        ),
        (
            "new key signs",
            vec![
                epoch.upgrade.keys[1].clone(),
                epoch.upgrade.keys[2].clone(),
                new_key(0xa0),
            ],
        ),
    ];
    let mut unsorted = epoch.upgrade.keys[1..4].to_vec();
    unsorted.reverse();
    signer_sets.push(("unsorted", unsorted));
    let mut repeated = epoch.upgrade.keys[1..3].to_vec();
    repeated.push(epoch.upgrade.keys[2].clone());
    signer_sets.push(("repeated", repeated));
    for (name, signers) in signer_sets {
        let mut control = good.clone();
        sign(&mut control, &signers);
        assert!(
            plan(&policies, None, 10, &control, &helper).is_err(),
            "{name}"
        );
    }
    let encodings: Vec<(&str, Change)> = vec![
        (
            "short signature",
            Box::new(|c| c.signatures[0].signature_base64 = B64.encode([1u8; 64])),
        ),
        (
            "hex signature",
            Box::new(|c| c.signatures[0].signature_base64 = hex::encode([1u8; SIGNATURE_BYTES])),
        ),
        (
            "short proof",
            Box::new(|c| c.proofs.freeze = B64.encode([1u8; 64])),
        ),
        (
            "kind",
            Box::new(|c| c.kind = upgrade::v2::CONTROL_KIND.into()),
        ),
    ];
    for (name, change) in encodings {
        let mut control = good.clone();
        change(&mut control);
        assert!(
            plan(&policies, None, 10, &control, &helper).is_err(),
            "{name}"
        );
    }
    // Context: the window and the anchor the adapter read.
    for (name, context) in [
        (
            "before the window",
            BlockContext {
                height: 9,
                ..context(10)
            },
        ),
        (
            "after the window",
            BlockContext {
                height: 13,
                ..context(10)
            },
        ),
        (
            "no anchor",
            BlockContext {
                finalized_anchor: None,
                ..context(10)
            },
        ),
        (
            "other anchor hash",
            BlockContext {
                finalized_anchor: Some(FinalizedAnchor {
                    height: 9,
                    app_hash: "cd".repeat(32),
                }),
                ..context(10)
            },
        ),
    ] {
        let bytes = serde_json::to_vec(&good).unwrap();
        assert!(
            plan_block(&policies, None, &context, &bytes, Some(&helper)).is_err(),
            "{name}"
        );
    }
    // An anchor older than the age bound.
    let age = policies.max_anchor_age_blocks();
    let mut old = control(&policies, &record, age + 2, 0xa0);
    old.payload.anchor_height = 0;
    sign(&mut old, &record.epochs[0].upgrade.keys[1..4]);
    let stale = BlockContext {
        height: age + 2,
        finalized_anchor: Some(FinalizedAnchor {
            height: 0,
            app_hash: "ab".repeat(32),
        }),
    };
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(plan_block(&policies, None, &stale, &bytes, Some(&helper)).is_err());
    // None of these reached the helper.
    assert!(helper.calls.lock().unwrap().is_empty());
    assert!(plan(&policies, None, 10, &good, &helper).is_ok());
}

#[test]
fn a_false_signature_or_proof_is_refused_and_a_helper_failure_is_an_error() {
    let config = config();
    let policies = policies(&config);
    let record = policies.current(None);
    let good = control(&policies, &record, 10, 0xa0);
    let helper = Helper::default();
    let mut forged = good.clone();
    forged.signatures[1].signature_base64 = forged.proofs.upgrade.clone();
    let error = plan(&policies, None, 10, &forged, &helper).unwrap_err();
    assert!(
        error.to_string().contains("signature rejected"),
        "{error:#}"
    );
    // A proof made by another key does not show possession of the new one.
    for role in 0..3 {
        let mut forged = good.clone();
        let artifact = artifact_bytes(&forged.payload).unwrap();
        let other = signed(&new_key(0x01).public_key_hex, &artifact);
        *[
            &mut forged.proofs.upgrade,
            &mut forged.proofs.freeze,
            &mut forged.proofs.resume,
        ][role] = other;
        let error = plan(&policies, None, 10, &forged, &helper).unwrap_err();
        assert!(
            error.to_string().contains("proof of possession"),
            "{error:#}"
        );
    }
    // Signatures and proofs bind the whole payload.
    let mut changed = good.clone();
    changed.payload.keys.freeze = new_key(0x01);
    assert!(plan(&policies, None, 10, &changed, &helper).is_err());
    let unavailable = Helper {
        unavailable: true,
        ..Helper::default()
    };
    let error = plan(&policies, None, 10, &good, &unavailable).unwrap_err();
    assert!(
        error.to_string().contains("helper unavailable"),
        "{error:#}"
    );
}

#[test]
fn one_replacement_at_a_time_and_the_next_is_signed_with_the_new_keys() {
    let config = config();
    let policies = policies(&config);
    let helper = Helper::default();
    let configured = policies.current(None);
    let first = control(&policies, &configured, 10, 0xa0);
    let record = plan(&policies, None, 10, &first, &helper).unwrap().record;
    let effect = record.epochs[1].from_height;
    // While the first is pending, no other is admitted.
    let second = control(&policies, &record, 11, 0xb0);
    let error = plan(&policies, Some(&record), 11, &second, &helper).unwrap_err();
    assert!(error.to_string().contains("pending"), "{error:#}");
    let late = control(&policies, &record, effect - 1, 0xb0);
    assert!(plan(&policies, Some(&record), effect - 1, &late, &helper).is_err());
    // From the effect height the old epoch's signatures are refused, under
    // either epoch number, and the new keys sign.
    let mut old = control(&policies, &record, effect, 0xb0);
    old.payload.authority_epoch = 1;
    old.payload.sequence = 2;
    sign(&mut old, &configured.epochs[0].upgrade.keys[1..4]);
    assert!(plan(&policies, Some(&record), effect, &old, &helper).is_err());
    let mut stale = control(&policies, &record, effect, 0xb0);
    let replaced = &first.payload.replaced.upgrade;
    let leaving = configured.epochs[0]
        .upgrade
        .keys
        .iter()
        .find(|k| &k.key_id == replaced)
        .unwrap()
        .clone();
    let mut signers = vec![
        leaving,
        record.epochs[1]
            .upgrade
            .keys
            .iter()
            .find(|k| k.key_id != *replaced && k.key_id != first.payload.keys.upgrade.key_id)
            .unwrap()
            .clone(),
    ];
    signers.push(first.payload.keys.upgrade.clone());
    signers.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    sign(&mut stale, &signers);
    assert!(plan(&policies, Some(&record), effect, &stale, &helper).is_err());
    let next = control(&policies, &record, effect, 0xb0);
    let planned = plan(&policies, Some(&record), effect, &next, &helper).unwrap();
    assert_eq!(planned.record.epochs.len(), 3);
    assert_eq!(planned.record.epochs[2].authority_epoch, 3);
    // A key that left never returns.
    let mut back = control(
        &policies,
        &planned.record,
        planned.record.epochs[2].from_height,
        0xc0,
    );
    back.payload.keys.upgrade = configured.epochs[0]
        .upgrade
        .keys
        .iter()
        .find(|k| &k.key_id == replaced)
        .unwrap()
        .clone();
    let signers = planned.record.epochs[2].upgrade.keys[1..4].to_vec();
    sign(&mut back, &signers);
    let height = planned.record.epochs[2].from_height;
    let error = plan(&policies, Some(&planned.record), height, &back, &helper).unwrap_err();
    assert!(error.to_string().contains("reuses a key"), "{error:#}");
}

#[test]
fn a_full_control_fits_the_transaction_bound_only_in_base64() {
    let config = config();
    let policies = policies(&config);
    let record = policies.current(None);
    let mut control = control(&policies, &record, u64::MAX / 2, 0xa0);
    // The most signatures a control may carry.
    while control.signatures.len() < policies.upgrade_v2.max_signatures {
        control.signatures.push(control.signatures[0].clone());
    }
    let bytes = serde_json::to_vec(&control).unwrap();
    assert!(
        bytes.len() <= policies.upgrade_v2.max_control_bytes,
        "{}",
        bytes.len()
    );
    assert!(bytes.len() <= config.max_tx_bytes, "{}", bytes.len());
    assert!(decode_control(&policies, &bytes).is_ok());
    // In hex, as the other controls carry signatures, six do not fit.
    assert!(6 * 2 * SIGNATURE_BYTES > config.max_tx_bytes);
}
