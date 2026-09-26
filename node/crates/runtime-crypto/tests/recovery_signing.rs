//! Local signed recovery-adapter correctness tests. These tests do not establish
//! live RPC enforcement, database atomicity, sponsor fees, or custody integration.

use dytallix_protocol_types::recovery::*;
use dytallix_protocol_types::recovery_wire::{
    decode, encode, signing_bytes, RecoveryOperation, RecoverySignature, SignatureRole,
    SignedRecovery,
};
use dytallix_runtime_crypto::recovery::{verify_and_transition, verify_signed};

fn domain() -> RecoveryDomain {
    RecoveryDomain {
        network: 3,
        chain_id: "signed-recovery-fixture".to_owned(),
        genesis_digest: [17; 32],
        account_id: [18; 32],
    }
}

fn role_order(role: &SignatureRole) -> u8 {
    match role {
        SignatureRole::Operation => 0,
        SignatureRole::Possession => 1,
    }
}

fn canonical_signatures(signatures: &mut [RecoverySignature]) {
    signatures.sort_by(|a, b| {
        role_order(&a.role)
            .cmp(&role_order(&b.role))
            .then_with(|| a.key.cmp(&b.key))
    });
}

#[cfg(feature = "pqc-fips204")]
mod real {
    use super::*;
    use fips204::ml_dsa_65;
    #[cfg(feature = "mldsa87-development")]
    use fips204::ml_dsa_87;
    use fips204::traits::{KeyGen, SerDes, Signer};
    use rand_core::OsRng;
    use std::collections::BTreeMap;

    /// Export public development evidence only. No secret key leaves this process.
    #[cfg(feature = "mldsa87-development")]
    #[test]
    #[ignore = "explicit public fixture regeneration"]
    fn export_public_development_fixture() {
        let directory =
            std::env::var("DYTALLIX_PUBLIC_FIXTURE_DIR").expect("explicit fixture directory");
        let value = Fixture::new("mldsa87").enroll();
        let bytes = encode(&value).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("recovery_signing.bin"),
            bytes,
        )
        .unwrap();
    }

    fn secondary_algorithm() -> &'static str {
        if cfg!(feature = "mldsa87-development") {
            "mldsa87"
        } else {
            "mldsa65"
        }
    }

    enum Secret {
        MlDsa65(ml_dsa_65::PrivateKey),
        #[cfg(feature = "mldsa87-development")]
        MlDsa87(ml_dsa_87::PrivateKey),
    }

    struct SigningKey {
        identity: KeyIdentity,
        secret: Secret,
    }

    impl SigningKey {
        fn generate(algorithm: &str) -> Self {
            match algorithm {
                "mldsa65" => {
                    let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut OsRng).unwrap();
                    Self {
                        identity: KeyIdentity {
                            algorithm: algorithm.to_owned(),
                            public_key: public.into_bytes().to_vec(),
                        },
                        secret: Secret::MlDsa65(secret),
                    }
                }
                #[cfg(feature = "mldsa87-development")]
                "mldsa87" => {
                    let (public, secret) = ml_dsa_87::KG::try_keygen_with_rng(&mut OsRng).unwrap();
                    Self {
                        identity: KeyIdentity {
                            algorithm: algorithm.to_owned(),
                            public_key: public.into_bytes().to_vec(),
                        },
                        secret: Secret::MlDsa87(secret),
                    }
                }
                _ => panic!("unsupported test algorithm"),
            }
        }

        fn sign(&self, operation: &RecoveryOperation, role: SignatureRole) -> RecoverySignature {
            let bytes = signing_bytes(operation, role.clone(), &self.identity).unwrap();
            let signature = match &self.secret {
                Secret::MlDsa65(secret) => secret.try_sign(&bytes, &[]).unwrap().to_vec(),
                #[cfg(feature = "mldsa87-development")]
                Secret::MlDsa87(secret) => secret.try_sign(&bytes, &[]).unwrap().to_vec(),
            };
            RecoverySignature {
                role,
                key: self.identity.clone(),
                signature,
            }
        }
    }

    struct Fixture {
        state: RecoveryState,
        active: SigningKey,
        guardians: Vec<SigningKey>,
        replacement: SigningKey,
    }

    impl Fixture {
        fn new(algorithm: &str) -> Self {
            let active = SigningKey::generate(algorithm);
            let mut guardians: Vec<_> = (0..3).map(|_| SigningKey::generate(algorithm)).collect();
            guardians.sort_by(|a, b| a.identity.cmp(&b.identity));
            let replacement = SigningKey::generate(algorithm);
            let config = RecoveryConfig {
                timing_version: 1,
                recovery_delay: 4,
                finalization_window: 3,
                policy_delay: 2,
                policy_window: 3,
                submission_lifetime: 20,
                algorithms: BTreeMap::from([
                    ("mldsa65".to_owned(), ml_dsa_65::PK_LEN),
                    ("mldsa87".to_owned(), 2592),
                ]),
            };
            let state = RecoveryState::new(domain(), config, active.identity.clone(), 1).unwrap();
            Self {
                state,
                active,
                guardians,
                replacement,
            }
        }

        fn operation(&self, kind: ActionKind) -> RecoveryOperation {
            RecoveryOperation {
                domain: domain(),
                action: Action {
                    submission_expiry: self.state.last_height + 10,
                    kind,
                },
            }
        }

        fn policy(&self) -> RecoveryPolicy {
            RecoveryPolicy {
                threshold: 2,
                guardians: self
                    .guardians
                    .iter()
                    .enumerate()
                    .map(|(i, guardian)| Guardian {
                        key: guardian.identity.clone(),
                        control_group: format!("independent-fixture-{i}"),
                    })
                    .collect(),
            }
        }

        fn enroll(&self) -> SignedRecovery {
            let operation = self.operation(ActionKind::Enroll {
                active: ActiveAuthorization {
                    generation: self.state.active_generation,
                    nonce: self.state.spending_nonce,
                },
                policy: self.policy(),
            });
            let mut signatures = vec![self.active.sign(&operation, SignatureRole::Operation)];
            signatures.extend(
                self.guardians
                    .iter()
                    .map(|guardian| guardian.sign(&operation, SignatureRole::Possession)),
            );
            canonical_signatures(&mut signatures);
            SignedRecovery {
                operation,
                signatures,
            }
        }

        fn enrolled(algorithm: &str) -> Self {
            let mut fixture = Self::new(algorithm);
            let wire = encode(&fixture.enroll()).unwrap();
            fixture.state = verify_and_transition(&fixture.state, 1, &wire).unwrap();
            fixture
        }

        fn start_operation(&self) -> RecoveryOperation {
            self.operation(ActionKind::Start {
                recovery: RecoveryAuthorization {
                    policy_version: self.state.policy_version,
                    sequence: self.state.recovery_sequence,
                },
                request_id: [19; 32],
                replacement: self.replacement.identity.clone(),
                timing_version: 1,
            })
        }

        fn start(&self) -> SignedRecovery {
            let operation = self.start_operation();
            let mut signatures = vec![
                self.guardians[0].sign(&operation, SignatureRole::Operation),
                self.guardians[1].sign(&operation, SignatureRole::Operation),
                self.replacement.sign(&operation, SignatureRole::Possession),
            ];
            canonical_signatures(&mut signatures);
            SignedRecovery {
                operation,
                signatures,
            }
        }

        fn assert_rejected(&self, signed: &SignedRecovery) {
            let original = self.state.clone();
            if let Ok(bytes) = encode(signed) {
                assert!(
                    verify_and_transition(&self.state, self.state.last_height, &bytes).is_err()
                );
            } else {
                assert!(verify_signed(signed).is_err());
            }
            assert_eq!(self.state, original);
        }
    }

    #[test]
    fn compiled_algorithms_enroll_start_and_finalize() {
        for algorithm in ["mldsa65", "mldsa87"]
            .into_iter()
            .filter(|a| *a == "mldsa65" || cfg!(feature = "mldsa87-development"))
        {
            let mut fixture = Fixture::new(algorithm);
            let enrolled = fixture.enroll();
            assert_eq!(enrolled.signatures.len(), 4);
            let wire = encode(&enrolled).unwrap();
            assert_eq!(encode(&decode(&wire).unwrap()).unwrap(), wire);
            let facts = verify_signed(&enrolled).unwrap();
            assert_eq!(facts.signers, vec![fixture.active.identity.clone()]);
            assert_eq!(facts.proofs.len(), 3);
            fixture.state = verify_and_transition(&fixture.state, 1, &wire).unwrap();
            let original = fixture.state.clone();
            let start = fixture.start();
            let wire = encode(&start).unwrap();
            fixture.state = verify_and_transition(&fixture.state, 1, &wire).unwrap();
            assert_eq!(fixture.state.status, RecoveryStatus::PendingRecovery);
            assert!(!fixture.state.outgoing_allowed());
            assert_eq!(fixture.state.spending_nonce, original.spending_nonce);
            assert_eq!(fixture.state.policy, original.policy);
            fixture.state = fixture.state.advance_height(5).unwrap();
            let operation = fixture.operation(ActionKind::Finalize {
                recovery: RecoveryAuthorization {
                    policy_version: fixture.state.policy_version,
                    sequence: fixture.state.recovery_sequence,
                },
                request_id: [19; 32],
            });
            let signature = fixture
                .replacement
                .sign(&operation, SignatureRole::Operation);
            let signed = SignedRecovery {
                operation,
                signatures: vec![signature],
            };
            let facts = verify_signed(&signed).unwrap();
            assert_eq!(facts.signers, vec![fixture.replacement.identity.clone()]);
            assert!(facts.proofs.is_empty());
            let finalized =
                verify_and_transition(&fixture.state, 5, &encode(&signed).unwrap()).unwrap();
            assert_eq!(finalized.status, RecoveryStatus::Normal);
            assert_eq!(finalized.active_key, fixture.replacement.identity);
            assert_eq!(finalized.domain, original.domain);
            assert_eq!(finalized.policy, original.policy);
            assert_eq!(finalized.spending_nonce, original.spending_nonce);
        }
    }

    #[test]
    fn signed_action_and_domain_tampering_reject_without_state_changes() {
        let fixture = Fixture::enrolled("mldsa65");
        let original = fixture.start();
        let mut mutations = Vec::new();
        let mut value = original.clone();
        value.operation.domain.chain_id.push_str("-other");
        mutations.push(value);
        let mut value = original.clone();
        value.operation.domain.account_id[0] ^= 1;
        mutations.push(value);
        let mut value = original.clone();
        value.operation.domain.genesis_digest[0] ^= 1;
        mutations.push(value);
        let mut value = original.clone();
        value.operation.domain.network = 2;
        mutations.push(value);
        let mut value = original.clone();
        value.operation.action.submission_expiry += 1;
        mutations.push(value);
        let mut value = original.clone();
        if let ActionKind::Start { request_id, .. } = &mut value.operation.action.kind {
            request_id[0] ^= 1;
        }
        mutations.push(value);
        for changed in mutations {
            assert!(verify_signed(&changed).is_err());
            fixture.assert_rejected(&changed);
        }
    }

    #[test]
    fn signature_role_key_and_signature_bytes_are_bound() {
        let fixture = Fixture::enrolled(secondary_algorithm());
        let original = fixture.start();
        let mut bad_signature = original.clone();
        bad_signature.signatures[0].signature[0] ^= 1;
        assert!(verify_signed(&bad_signature).is_err());
        fixture.assert_rejected(&bad_signature);
        let mut bad_key = original.clone();
        bad_key.signatures[0].key = fixture.guardians[2].identity.clone();
        canonical_signatures(&mut bad_key.signatures);
        assert!(verify_signed(&bad_key).is_err());
        fixture.assert_rejected(&bad_key);
        let mut bad_role = original.clone();
        let proof = bad_role
            .signatures
            .iter_mut()
            .find(|s| matches!(s.role, SignatureRole::Possession))
            .unwrap();
        proof.role = SignatureRole::Operation;
        canonical_signatures(&mut bad_role.signatures);
        assert!(verify_signed(&bad_role).is_err());
        fixture.assert_rejected(&bad_role);
        let mut alias = original.clone();
        alias.signatures[0].key.algorithm = "dilithium5".to_owned();
        canonical_signatures(&mut alias.signatures);
        assert!(verify_signed(&alias).is_err());
        fixture.assert_rejected(&alias);
    }

    #[test]
    fn valid_crypto_does_not_grant_unauthorized_guardian_power() {
        let fixture = Fixture::enrolled("mldsa65");
        let outsider = SigningKey::generate("mldsa65");
        let operation = fixture.start_operation();
        let mut signatures = vec![
            outsider.sign(&operation, SignatureRole::Operation),
            fixture.guardians[0].sign(&operation, SignatureRole::Operation),
            fixture
                .replacement
                .sign(&operation, SignatureRole::Possession),
        ];
        canonical_signatures(&mut signatures);
        let signed = SignedRecovery {
            operation,
            signatures,
        };
        assert!(
            verify_signed(&signed).is_ok(),
            "all signatures are authentic"
        );
        fixture.assert_rejected(&signed);
        let operation = fixture.start_operation();
        let mut signatures = vec![
            fixture.guardians[0].sign(&operation, SignatureRole::Operation),
            fixture
                .replacement
                .sign(&operation, SignatureRole::Possession),
        ];
        canonical_signatures(&mut signatures);
        let one_guardian = SignedRecovery {
            operation,
            signatures,
        };
        assert!(verify_signed(&one_guardian).is_ok());
        fixture.assert_rejected(&one_guardian);
        assert_eq!(fixture.state.status, RecoveryStatus::Normal);
    }

    #[test]
    fn stale_sequence_and_missing_replacement_proof_reject_after_crypto_verification() {
        let mut fixture = Fixture::enrolled("mldsa65");
        let mut missing_proof = fixture.start();
        missing_proof
            .signatures
            .retain(|s| matches!(s.role, SignatureRole::Operation));
        assert!(verify_signed(&missing_proof).is_ok());
        fixture.assert_rejected(&missing_proof);
        let start = fixture.start();
        fixture.state = verify_and_transition(&fixture.state, 1, &encode(&start).unwrap()).unwrap();
        fixture.state = fixture.state.advance_height(8).unwrap();
        assert_eq!(fixture.state.status, RecoveryStatus::RecoveryLocked);
        assert!(verify_signed(&start).is_ok());
        fixture.assert_rejected(&start);
    }

    #[test]
    fn duplicate_unordered_and_noncanonical_envelopes_reject() {
        let fixture = Fixture::enrolled("mldsa65");
        let original = fixture.start();
        let mut duplicate = original.clone();
        duplicate.signatures.push(duplicate.signatures[0].clone());
        canonical_signatures(&mut duplicate.signatures);
        assert!(encode(&duplicate).is_err());
        assert!(verify_signed(&duplicate).is_err());
        let mut unordered = original.clone();
        unordered.signatures.swap(0, 1);
        assert!(encode(&unordered).is_err());
        assert!(verify_signed(&unordered).is_err());
        let bytes = encode(&original).unwrap();
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        assert!(verify_and_transition(&fixture.state, 1, &trailing).is_err());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut unsorted_guardians = fixture.enroll();
        if let ActionKind::Enroll { policy, .. } = &mut unsorted_guardians.operation.action.kind {
            policy.guardians.swap(0, 1);
        }
        assert!(encode(&unsorted_guardians).is_err());
    }

    #[test]
    fn generic_spend_action_is_not_a_recovery_signing_authorization() {
        let fixture = Fixture::enrolled("mldsa65");
        let operation = fixture.operation(ActionKind::Spend {
            active: ActiveAuthorization {
                generation: fixture.state.active_generation,
                nonce: fixture.state.spending_nonce,
            },
        });
        assert!(signing_bytes(
            &operation,
            SignatureRole::Operation,
            &fixture.active.identity
        )
        .is_err());
        let signed = SignedRecovery {
            operation,
            signatures: vec![],
        };
        assert!(encode(&signed).is_err());
        assert!(verify_signed(&signed).is_err());
        fixture.assert_rejected(&signed);
    }
}

#[cfg(not(feature = "pqc-fips204"))]
#[test]
fn absent_real_backend_rejects_nonempty_mockish_signatures() {
    for (algorithm, public_len, signature_len) in [("mldsa65", 1952, 3309), ("mldsa87", 2592, 4627)]
    {
        let key = |byte| KeyIdentity {
            algorithm: algorithm.to_owned(),
            public_key: vec![byte; public_len],
        };
        let active = key(1);
        let operation = RecoveryOperation {
            domain: domain(),
            action: Action {
                submission_expiry: 11,
                kind: ActionKind::Enroll {
                    active: ActiveAuthorization {
                        generation: 0,
                        nonce: 0,
                    },
                    policy: RecoveryPolicy {
                        threshold: 2,
                        guardians: (2..=4)
                            .map(|byte| Guardian {
                                key: key(byte),
                                control_group: format!("fixture-{byte}"),
                            })
                            .collect(),
                    },
                },
            },
        };
        let mut signatures = vec![RecoverySignature {
            role: SignatureRole::Operation,
            key: active,
            signature: vec![0xA5; signature_len],
        }];
        signatures.extend((2..=4).map(|byte| RecoverySignature {
            role: SignatureRole::Possession,
            key: key(byte),
            signature: vec![0xA5; signature_len],
        }));
        canonical_signatures(&mut signatures);
        let signed = SignedRecovery {
            operation,
            signatures,
        };
        let wire = encode(&signed).expect("bounded canonical input reaches the backend gate");
        let decoded = decode(&wire).unwrap();
        assert!(matches!(
            verify_signed(&decoded),
            Err(dytallix_runtime_crypto::recovery::RecoveryVerificationError::BackendUnavailable)
        ));
    }
}

/// The same valid public development signature must fail in the strict backend.
#[cfg(feature = "pqc-fips204")]
#[test]
fn frozen_mldsa87_signature_matches_explicit_backend_selection() {
    let bytes = include_bytes!("fixtures/mldsa87/recovery_signing.bin");
    let value = decode(bytes).unwrap();
    let result = verify_signed(&value);
    #[cfg(feature = "mldsa87-development")]
    assert!(
        result.is_ok(),
        "public development fixture must verify: {result:?}"
    );
    #[cfg(not(feature = "mldsa87-development"))]
    assert!(matches!(
        result,
        Err(dytallix_runtime_crypto::recovery::RecoveryVerificationError::UnsupportedAlgorithm)
    ));
}
