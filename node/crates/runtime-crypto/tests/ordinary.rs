//! Complete-body authenticity tests. These do not grant current account authority
//! or select ordinary fees. All profile values here are explicit local fixtures.

use dytallix_protocol_types::{
    ordinary::{self as wire, Action, Denomination, Limits, OrdinaryTransaction, SignedOrdinary},
    recovery::{KeyIdentity, RecoveryDomain},
};
use dytallix_runtime_crypto::ordinary::{verify_bytes, verify_signed};
use std::collections::BTreeSet;

fn limits() -> Limits {
    Limits {
        max_wire_bytes: 65_536,
        max_actions: 16,
        max_identifier_bytes: 64,
        max_data_bytes: 1024,
        max_memo_bytes: 128,
        max_consensus_key_bytes: 4096,
        max_proof_bytes: 8192,
        max_expiry_lifetime: 100,
        allowed_algorithms: BTreeSet::from(["mldsa65".into(), "mldsa87".into()]),
    }
}

fn body(key: KeyIdentity) -> OrdinaryTransaction {
    OrdinaryTransaction {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "ordinary-signature-fixture".into(),
            genesis_digest: [1; 32],
            account_id: [2; 32],
        },
        authorization_generation: 3,
        spending_nonce: 4,
        key,
        expiry_height: 80,
        ordinary_fee_contract_version: 1,
        fee_profile_version: 6,
        fee_profile_digest: [7; 32],
        fee_denomination: Denomination::Udrt,
        maximum_fee: 50_000,
        gas_limit: 10_000,
        memo: "complete ordered operation list".into(),
        actions: vec![
            Action::Send {
                recipient: [8; 32],
                denomination: Denomination::Udgt,
                amount: 9,
            },
            Action::Data {
                data: "fixture data".into(),
            },
            Action::DmsRegister {
                beneficiary: [10; 32],
                period_blocks: 11,
            },
            Action::DmsPing,
            Action::DmsClaim {
                owner: [12; 32],
                expected_grant_generation: 13,
            },
            Action::RewardBond {
                validator_id: "validator-a".into(),
                amount_udgt: 14,
            },
            Action::RewardBeginUnbond {
                validator_id: "validator-a".into(),
                amount_udgt: 15,
            },
            Action::RewardClaim,
            Action::ValidatorRegister {
                validator_id: "validator-b".into(),
                consensus_key: vec![16; 32],
                proof: vec![17; 64],
                proof_expiry_height: 70,
                amount_udgt: 18,
            },
            Action::ValidatorRotateKey {
                validator_id: "validator-c".into(),
                consensus_key: vec![19; 32],
                proof: vec![20; 64],
                proof_expiry_height: 71,
            },
            Action::ValidatorExit {
                validator_id: "validator-d".into(),
            },
            Action::ValidatorWithdraw {
                unbond_id: "unbond-e".into(),
            },
        ],
    }
}

#[cfg(feature = "pqc-fips204")]
mod real {
    use super::*;
    use dytallix_protocol_types::sha3_256;
    #[cfg(feature = "mldsa87-development")]
    use fips204::ml_dsa_87;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    use rand_core::OsRng;

    /// Export public development evidence only. No secret key leaves this process.
    #[cfg(feature = "mldsa87-development")]
    #[test]
    #[ignore = "explicit public fixture regeneration"]
    fn export_public_development_fixture() {
        let directory =
            std::env::var("DYTALLIX_PUBLIC_FIXTURE_DIR").expect("explicit fixture directory");
        let value = fixture("mldsa87").1;
        let bytes = wire::encode(&value, &limits()).unwrap();
        std::fs::write(std::path::Path::new(&directory).join("ordinary.bin"), bytes).unwrap();
    }

    fn secondary_algorithm() -> &'static str {
        if cfg!(feature = "mldsa87-development") {
            "mldsa87"
        } else {
            "mldsa65"
        }
    }

    enum Secret {
        Dsa65(ml_dsa_65::PrivateKey),
        #[cfg(feature = "mldsa87-development")]
        Dsa87(ml_dsa_87::PrivateKey),
    }
    struct Key {
        identity: KeyIdentity,
        secret: Secret,
    }
    impl Key {
        fn new(algorithm: &str) -> Self {
            match algorithm {
                "mldsa65" => {
                    let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut OsRng).unwrap();
                    Self {
                        identity: KeyIdentity {
                            algorithm: algorithm.into(),
                            public_key: public.into_bytes().to_vec(),
                        },
                        secret: Secret::Dsa65(secret),
                    }
                }
                #[cfg(feature = "mldsa87-development")]
                "mldsa87" => {
                    let (public, secret) = ml_dsa_87::KG::try_keygen_with_rng(&mut OsRng).unwrap();
                    Self {
                        identity: KeyIdentity {
                            algorithm: algorithm.into(),
                            public_key: public.into_bytes().to_vec(),
                        },
                        secret: Secret::Dsa87(secret),
                    }
                }
                _ => panic!("unsupported test algorithm"),
            }
        }
        fn sign_context(&self, bytes: &[u8], context: &[u8]) -> Vec<u8> {
            match &self.secret {
                Secret::Dsa65(key) => key.try_sign(bytes, context).unwrap().to_vec(),
                #[cfg(feature = "mldsa87-development")]
                Secret::Dsa87(key) => key.try_sign(bytes, context).unwrap().to_vec(),
            }
        }
        fn sign(&self, body: OrdinaryTransaction) -> SignedOrdinary {
            let bytes = wire::signing_bytes(&body, &limits()).unwrap();
            SignedOrdinary {
                body,
                signature: self.sign_context(&bytes, &[]),
            }
        }
    }
    fn fixture(algorithm: &str) -> (Key, SignedOrdinary) {
        let key = Key::new(algorithm);
        let signed = key.sign(body(key.identity.clone()));
        (key, signed)
    }

    #[test]
    fn mainnet_operational_profile_checks_compiled_algorithms() {
        for algorithm in ["mldsa65", "mldsa87"]
            .into_iter()
            .filter(|a| *a == "mldsa65" || cfg!(feature = "mldsa87-development"))
        {
            let key = Key::new(algorithm);
            let mut tx = body(key.identity.clone());
            tx.domain.network = 1;
            let signed = key.sign(tx);
            assert_eq!(
                verify_signed(&signed, &limits()).is_ok(),
                algorithm == "mldsa65"
            );
        }
    }

    #[test]
    fn compiled_algorithms_authenticate_all_twelve_actions_and_complete_body() {
        for algorithm in ["mldsa65", "mldsa87"]
            .into_iter()
            .filter(|a| *a == "mldsa65" || cfg!(feature = "mldsa87-development"))
        {
            let (_, signed) = fixture(algorithm);
            let bytes = wire::encode(&signed, &limits()).unwrap();
            let verified = verify_bytes(&bytes, &limits()).unwrap();
            assert_eq!(verified.body(), &signed.body);
            assert_eq!(verified.body().actions.len(), 12);
            assert_eq!(
                verified.transaction_id(),
                wire::transaction_id(&signed.body, &limits()).unwrap()
            );
            assert_eq!(
                verified.envelope_hash(),
                wire::envelope_hash(&signed, &limits()).unwrap()
            );
            assert_eq!(verify_signed(&signed, &limits()).unwrap(), verified);
        }
    }

    #[test]
    fn every_outer_authority_fee_and_domain_field_is_signed() {
        let (_, original) = fixture("mldsa65");
        let changes: Vec<fn(&mut OrdinaryTransaction)> = vec![
            |b| b.domain.network = 2,
            |b| b.domain.chain_id.push('x'),
            |b| b.domain.genesis_digest[0] ^= 1,
            |b| b.domain.account_id[0] ^= 1,
            |b| b.authorization_generation += 1,
            |b| b.spending_nonce += 1,
            |b| b.expiry_height += 1,
            |b| b.ordinary_fee_contract_version += 1,
            |b| b.fee_profile_version += 1,
            |b| b.fee_profile_digest[0] ^= 1,
            |b| b.maximum_fee += 1,
            |b| b.gas_limit += 1,
            |b| b.memo.push('x'),
            |b| {
                b.actions.pop();
            },
            |b| b.actions.swap(0, 1),
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let mut changed = original.clone();
            change(&mut changed.body);
            assert!(
                wire::encode(&changed, &limits()).is_ok(),
                "mutation {index} remains syntactically valid"
            );
            assert!(
                verify_signed(&changed, &limits()).is_err(),
                "unsigned mutation {index}"
            );
        }
        let mut changed = original;
        changed.body.fee_denomination = Denomination::Udgt;
        assert!(verify_signed(&changed, &limits()).is_err());
    }

    #[test]
    fn every_typed_action_payload_and_order_is_signed() {
        let (_, original) = fixture(secondary_algorithm());
        let changes: Vec<fn(&mut Vec<Action>)> = vec![
            |a| {
                if let Action::Send { recipient, .. } = &mut a[0] {
                    recipient[0] ^= 1
                }
            },
            |a| {
                if let Action::Send { denomination, .. } = &mut a[0] {
                    *denomination = Denomination::Udrt
                }
            },
            |a| {
                if let Action::Send { amount, .. } = &mut a[0] {
                    *amount += 1
                }
            },
            |a| {
                if let Action::Data { data } = &mut a[1] {
                    data.push('x')
                }
            },
            |a| {
                if let Action::DmsRegister { beneficiary, .. } = &mut a[2] {
                    beneficiary[0] ^= 1
                }
            },
            |a| {
                if let Action::DmsRegister { period_blocks, .. } = &mut a[2] {
                    *period_blocks += 1
                }
            },
            |a| a[3] = Action::RewardClaim,
            |a| {
                if let Action::DmsClaim { owner, .. } = &mut a[4] {
                    owner[0] ^= 1
                }
            },
            |a| {
                if let Action::DmsClaim {
                    expected_grant_generation,
                    ..
                } = &mut a[4]
                {
                    *expected_grant_generation += 1
                }
            },
            |a| {
                if let Action::RewardBond { validator_id, .. } = &mut a[5] {
                    validator_id.push('x')
                }
            },
            |a| {
                if let Action::RewardBond { amount_udgt, .. } = &mut a[5] {
                    *amount_udgt += 1
                }
            },
            |a| {
                if let Action::RewardBeginUnbond { validator_id, .. } = &mut a[6] {
                    validator_id.push('x')
                }
            },
            |a| {
                if let Action::RewardBeginUnbond { amount_udgt, .. } = &mut a[6] {
                    *amount_udgt += 1
                }
            },
            |a| a[7] = Action::DmsPing,
            |a| {
                if let Action::ValidatorRegister { validator_id, .. } = &mut a[8] {
                    validator_id.push('x')
                }
            },
            |a| {
                if let Action::ValidatorRegister { consensus_key, .. } = &mut a[8] {
                    consensus_key[0] ^= 1
                }
            },
            |a| {
                if let Action::ValidatorRegister { proof, .. } = &mut a[8] {
                    proof[0] ^= 1
                }
            },
            |a| {
                if let Action::ValidatorRegister {
                    proof_expiry_height,
                    ..
                } = &mut a[8]
                {
                    *proof_expiry_height += 1
                }
            },
            |a| {
                if let Action::ValidatorRegister { amount_udgt, .. } = &mut a[8] {
                    *amount_udgt += 1
                }
            },
            |a| {
                if let Action::ValidatorRotateKey { validator_id, .. } = &mut a[9] {
                    validator_id.push('x')
                }
            },
            |a| {
                if let Action::ValidatorRotateKey { consensus_key, .. } = &mut a[9] {
                    consensus_key[0] ^= 1
                }
            },
            |a| {
                if let Action::ValidatorRotateKey { proof, .. } = &mut a[9] {
                    proof[0] ^= 1
                }
            },
            |a| {
                if let Action::ValidatorRotateKey {
                    proof_expiry_height,
                    ..
                } = &mut a[9]
                {
                    *proof_expiry_height += 1
                }
            },
            |a| {
                if let Action::ValidatorExit { validator_id } = &mut a[10] {
                    validator_id.push('x')
                }
            },
            |a| {
                if let Action::ValidatorWithdraw { unbond_id } = &mut a[11] {
                    unbond_id.push('x')
                }
            },
            |a| a.swap(8, 9),
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let mut changed = original.clone();
            change(&mut changed.body.actions);
            assert_ne!(
                changed.body, original.body,
                "action mutation {index} applied"
            );
            assert!(wire::encode(&changed, &limits()).is_ok());
            assert!(
                verify_signed(&changed, &limits()).is_err(),
                "unsigned action mutation {index}"
            );
        }
    }

    #[test]
    fn wrong_key_signature_context_application_hash_and_domain_reject() {
        let (key, original) = fixture("mldsa65");
        let mut changed = original.clone();
        changed.body.key = Key::new("mldsa65").identity;
        assert!(verify_signed(&changed, &limits()).is_err());
        let mut changed = original.clone();
        changed.signature[0] ^= 1;
        assert!(verify_signed(&changed, &limits()).is_err());
        let message = wire::signing_bytes(&original.body, &limits()).unwrap();
        let mut changed = original.clone();
        changed.signature = key.sign_context(&message, b"nonempty-context");
        assert!(verify_signed(&changed, &limits()).is_err());
        let mut changed = original.clone();
        changed.signature = key.sign_context(&sha3_256(&message), &[]);
        assert!(verify_signed(&changed, &limits()).is_err());
        let mut wrong_domain = b"DYTALLIX/RECOVERY-SPONSOR\0".to_vec();
        wrong_domain.extend_from_slice(&message);
        let mut changed = original;
        changed.signature = key.sign_context(&wrong_domain, &[]);
        assert!(verify_signed(&changed, &limits()).is_err());
    }

    #[test]
    fn explicit_role_allowlist_and_limits_cannot_be_bypassed() {
        let (_, signed) = fixture(secondary_algorithm());
        let bytes = wire::encode(&signed, &limits()).unwrap();
        let mut selected = limits();
        selected.allowed_algorithms = BTreeSet::from([if signed.body.key.algorithm == "mldsa65" {
            "mldsa87".into()
        } else {
            "mldsa65".into()
        }]);
        assert!(verify_signed(&signed, &selected).is_err());
        assert!(verify_bytes(&bytes, &selected).is_err());
        let mut selected = limits();
        selected.max_actions = 11;
        assert!(verify_signed(&signed, &selected).is_err());
        let mut selected = limits();
        selected.max_wire_bytes = bytes.len() as u32 - 1;
        assert!(verify_bytes(&bytes, &selected).is_err());
        let mut selected = limits();
        selected.max_memo_bytes = 1;
        assert!(verify_signed(&signed, &selected).is_err());
        for alias in ["dilithium3", "dilithium5", "mock-blake3", "MLDSA87"] {
            let mut changed = signed.clone();
            changed.body.key.algorithm = alias.into();
            let mut selected = limits();
            selected.allowed_algorithms.insert(alias.into());
            assert!(verify_signed(&changed, &selected).is_err());
        }
    }

    #[test]
    fn canonical_envelope_framing_and_signature_lengths_are_required() {
        let (_, signed) = fixture("mldsa65");
        let bytes = wire::encode(&signed, &limits()).unwrap();
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(verify_bytes(&trailing, &limits()).is_err());
        assert!(verify_bytes(&bytes[..bytes.len() - 1], &limits()).is_err());
        let mut changed = signed.clone();
        changed.signature.pop();
        assert!(verify_signed(&changed, &limits()).is_err());
        let mut changed = signed;
        changed.body.key.public_key.pop();
        assert!(verify_signed(&changed, &limits()).is_err());
        assert!(verify_bytes(&vec![0; 65_537], &limits()).is_err());
    }

    #[test]
    fn resigning_preserves_transaction_id_and_verified_body_is_an_immutable_snapshot() {
        let (key, mut original) = fixture(secondary_algorithm());
        let verified = verify_signed(&original, &limits()).unwrap();
        let resigned = key.sign(original.body.clone());
        let second = verify_signed(&resigned, &limits()).unwrap();
        assert_eq!(verified.transaction_id(), second.transaction_id());
        assert_eq!(verified.body(), second.body());
        assert_ne!(original.signature, resigned.signature);
        assert_ne!(verified.envelope_hash(), second.envelope_hash());
        original.body.spending_nonce += 1;
        original.body.actions.clear();
        assert_eq!(verified.body(), &resigned.body);
        assert_eq!(verified.clone(), verified);
    }
}

#[cfg(not(feature = "pqc-fips204"))]
#[test]
fn missing_real_backend_rejects_canonical_input_even_with_mock_enabled() {
    use dytallix_runtime_crypto::ordinary::OrdinaryVerificationError;
    for (algorithm, public_len, signature_len) in [("mldsa65", 1952, 3309), ("mldsa87", 2592, 4627)]
    {
        let signed = SignedOrdinary {
            body: body(KeyIdentity {
                algorithm: algorithm.into(),
                public_key: vec![1; public_len],
            }),
            signature: vec![2; signature_len],
        };
        let bytes = wire::encode(&signed, &limits()).expect("canonical input reaches backend gate");
        assert!(matches!(
            verify_signed(&signed, &limits()),
            Err(OrdinaryVerificationError::BackendUnavailable)
        ));
        assert!(matches!(
            verify_bytes(&bytes, &limits()),
            Err(OrdinaryVerificationError::BackendUnavailable)
        ));
    }
}

/// The same valid public development signature must fail in the strict backend.
#[cfg(feature = "pqc-fips204")]
#[test]
fn frozen_mldsa87_signature_matches_explicit_backend_selection() {
    let bytes = include_bytes!("fixtures/mldsa87/ordinary.bin");
    let value = wire::decode(bytes, &limits()).unwrap();
    let result = verify_signed(&value, &limits());
    #[cfg(feature = "mldsa87-development")]
    assert!(
        result.is_ok(),
        "public development fixture must verify: {result:?}"
    );
    #[cfg(not(feature = "mldsa87-development"))]
    assert!(matches!(
        result,
        Err(dytallix_runtime_crypto::ordinary::OrdinaryVerificationError::UnsupportedAlgorithm)
    ));
    let message = wire::signing_bytes(&value.body, &limits()).unwrap();
    let general = dytallix_runtime_crypto::verify(
        &value.body.key.public_key,
        &message,
        &value.signature,
        dytallix_runtime_crypto::PQCAlgorithm::MlDsa87,
    );
    #[cfg(feature = "mldsa87-development")]
    assert!(
        general.is_ok(),
        "general verifier must verify this valid fixture"
    );
    #[cfg(not(feature = "mldsa87-development"))]
    assert!(matches!(
        general,
        Err(dytallix_runtime_crypto::PQCVerifyError::UnsupportedAlgorithm(_))
    ));
}
