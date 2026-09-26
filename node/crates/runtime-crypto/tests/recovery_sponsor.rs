//! Local signature and binding checks. Current account and fee authority belong
//! to execution. These tests do not qualify production custody or deployment.

use dytallix_protocol_types::{
    recovery::{Action, ActionKind, KeyIdentity, RecoveryAuthorization, RecoveryDomain},
    recovery_sponsor::{self as wire, SponsorAuthorization, SponsoredRecovery},
    recovery_wire::{self, RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery},
};
use dytallix_runtime_crypto::recovery_sponsor::{verify_bytes, verify_signed};

fn operation() -> RecoveryOperation {
    RecoveryOperation {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "sponsor-signature-fixture".to_owned(),
            genesis_digest: [1; 32],
            account_id: [2; 32],
        },
        action: Action {
            submission_expiry: 30,
            kind: ActionKind::Finalize {
                recovery: RecoveryAuthorization {
                    policy_version: 1,
                    sequence: 2,
                },
                request_id: [3; 32],
            },
        },
    }
}

fn authorization(recovery: &SignedRecovery, key: KeyIdentity) -> SponsorAuthorization {
    SponsorAuthorization {
        domain: recovery.operation.domain.clone(),
        recovery_version: recovery_wire::VERSION,
        operation_id: wire::operation_id(&recovery.operation).unwrap(),
        signer_manifest_digest: wire::signer_manifest_digest(recovery).unwrap(),
        sponsor_account_id: [4; 32],
        sponsor_generation: 7,
        sponsor_nonce: 8,
        sponsor_key: key,
        fee_profile_version: 9,
        fee_profile_digest: [10; 32],
        denomination: "udrt".to_owned(),
        maximum_charge: 20_000,
        gas_limit: 1_000,
        expiry_height: 25,
    }
}

mod real {
    use super::*;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    use rand_core::OsRng;


    fn secondary_algorithm() -> &'static str {
        "mldsa65"
    }

    enum Secret {
        Dsa65(ml_dsa_65::PrivateKey),
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
                _ => panic!("unsupported test key"),
            }
        }

        fn sign_context(&self, message: &[u8], context: &[u8]) -> Vec<u8> {
            match &self.secret {
                Secret::Dsa65(key) => key.try_sign(message, context).unwrap().to_vec(),
            }
        }

        fn sign(&self, message: &[u8]) -> Vec<u8> {
            self.sign_context(message, &[])
        }

        fn inner(&self, operation: &RecoveryOperation, role: SignatureRole) -> RecoverySignature {
            RecoverySignature {
                role,
                key: self.identity.clone(),
                signature: self
                    .sign(&recovery_wire::signing_bytes(operation, role, &self.identity).unwrap()),
            }
        }

        fn sponsor(&self, value: &mut SponsoredRecovery) {
            value.signature = self.sign(&wire::sponsor_signing_bytes(&value.sponsor).unwrap());
        }
    }

    fn fixture(inner_algorithm: &str, sponsor_algorithm: &str) -> (Key, Key, SponsoredRecovery) {
        let inner = Key::new(inner_algorithm);
        let sponsor = Key::new(sponsor_algorithm);
        let operation = operation();
        let recovery = SignedRecovery {
            signatures: vec![inner.inner(&operation, SignatureRole::Operation)],
            operation,
        };
        let mut value = SponsoredRecovery {
            sponsor: authorization(&recovery, sponsor.identity.clone()),
            recovery,
            signature: vec![],
        };
        sponsor.sponsor(&mut value);
        (inner, sponsor, value)
    }

    #[test]
    fn mainnet_recovery_and_sponsor_roles_require_mldsa65() {
        for (inner_alg, sponsor_alg) in [("mldsa65", "mldsa65")] {
            let (inner, sponsor, mut value) = fixture(inner_alg, sponsor_alg);
            value.recovery.operation.domain.network = 1;
            value.recovery.signatures =
                vec![inner.inner(&value.recovery.operation, SignatureRole::Operation)];
            value.sponsor = authorization(&value.recovery, sponsor.identity.clone());
            sponsor.sponsor(&mut value);
            assert_eq!(
                verify_signed(&value).is_ok(),
                inner_alg == "mldsa65" && sponsor_alg == "mldsa65"
            );
        }
    }

    #[test]
    fn compiled_sponsor_algorithms_verify_without_gaining_recovery_roles() {
        for (inner_algorithm, sponsor_algorithm) in [("mldsa65", "mldsa65")] {
            let (inner, sponsor, value) = fixture(inner_algorithm, sponsor_algorithm);
            let encoded = wire::encode(&value).unwrap();
            let facts = verify_bytes(&encoded).unwrap();
            assert_eq!(facts.domain, value.recovery.operation.domain);
            assert_eq!(facts.action, value.recovery.operation.action);
            assert_eq!(facts.signers, vec![inner.identity]);
            assert!(facts.proofs.is_empty());
            assert!(!facts.signers.contains(&sponsor.identity));
            assert_eq!(verify_signed(&value).unwrap(), facts);
        }
    }

    #[test]
    fn every_sponsor_authorization_field_is_signed() {
        let (_, _, original) = fixture("mldsa65", "mldsa65");
        let changes: Vec<fn(&mut SponsorAuthorization)> = vec![
            |s| s.domain.network = 2,
            |s| s.domain.chain_id.push('x'),
            |s| s.domain.genesis_digest[0] ^= 1,
            |s| s.domain.account_id[0] ^= 1,
            |s| s.recovery_version += 1,
            |s| s.operation_id[0] ^= 1,
            |s| s.signer_manifest_digest[0] ^= 1,
            |s| s.sponsor_account_id[0] ^= 1,
            |s| s.sponsor_generation += 1,
            |s| s.sponsor_nonce += 1,
            |s| s.fee_profile_version += 1,
            |s| s.fee_profile_digest[0] ^= 1,
            |s| s.denomination = "udgt".into(),
            |s| s.maximum_charge += 1,
            |s| s.gas_limit += 1,
            |s| s.expiry_height += 1,
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let mut changed = original.clone();
            change(&mut changed.sponsor);
            assert!(verify_signed(&changed).is_err(), "field mutation {index}");
        }
    }

    #[test]
    fn valid_sponsor_signature_cannot_authorize_mismatched_inner_bindings() {
        let (_, sponsor, original) = fixture("mldsa65", "mldsa65");
        let changes: Vec<fn(&mut SponsorAuthorization)> = vec![
            |s| s.domain.network = 2,
            |s| s.domain.chain_id.push('x'),
            |s| s.domain.genesis_digest[0] ^= 1,
            |s| s.domain.account_id[0] ^= 1,
            |s| s.operation_id[0] ^= 1,
            |s| s.signer_manifest_digest[0] ^= 1,
        ];
        for change in changes {
            let mut changed = original.clone();
            change(&mut changed.sponsor);
            sponsor.sponsor(&mut changed);
            assert!(verify_signed(&changed).is_err());
        }
    }

    #[test]
    fn sponsor_cannot_replace_inner_operation_signatures_or_roles() {
        let (inner, sponsor, original) = fixture("mldsa65", secondary_algorithm());
        let mut changed = original.clone();
        changed.recovery.operation.action.submission_expiry += 1;
        changed.sponsor.operation_id = wire::operation_id(&changed.recovery.operation).unwrap();
        sponsor.sponsor(&mut changed);
        assert!(verify_signed(&changed).is_err());

        let mut changed = original.clone();
        changed.recovery.signatures[0].role = SignatureRole::Possession;
        changed.sponsor.signer_manifest_digest =
            wire::signer_manifest_digest(&changed.recovery).unwrap();
        sponsor.sponsor(&mut changed);
        assert!(verify_signed(&changed).is_err());

        let mut changed = original.clone();
        changed.recovery.signatures[0].signature[0] ^= 1;
        assert!(verify_signed(&changed).is_err());

        let mut changed = original.clone();
        changed
            .recovery
            .signatures
            .push(inner.inner(&changed.recovery.operation, SignatureRole::Possession));
        assert!(
            verify_signed(&changed).is_err(),
            "sponsor fixed the signer manifest"
        );
        changed.sponsor.signer_manifest_digest =
            wire::signer_manifest_digest(&changed.recovery).unwrap();
        sponsor.sponsor(&mut changed);
        let facts = verify_signed(&changed).unwrap();
        assert_eq!(facts.signers, vec![inner.identity.clone()]);
        assert_eq!(facts.proofs, vec![inner.identity]);
    }

    #[test]
    fn sponsor_key_signature_domain_context_and_aliases_reject() {
        let (_, sponsor, original) = fixture("mldsa65", "mldsa65");
        let outsider = Key::new("mldsa65");
        let mut changed = original.clone();
        changed.sponsor.sponsor_key = outsider.identity;
        assert!(verify_signed(&changed).is_err());
        let mut changed = original.clone();
        changed.signature[0] ^= 1;
        assert!(verify_signed(&changed).is_err());
        let mut changed = original.clone();
        changed.signature = sponsor.sign_context(
            &wire::sponsor_signing_bytes(&changed.sponsor).unwrap(),
            b"nonempty-context",
        );
        assert!(verify_signed(&changed).is_err());
        let mut changed = original.clone();
        changed.signature = sponsor.sign(
            &recovery_wire::signing_bytes(
                &changed.recovery.operation,
                SignatureRole::Operation,
                &sponsor.identity,
            )
            .unwrap(),
        );
        assert!(verify_signed(&changed).is_err());
        for alias in ["dilithium3", "dilithium5", "mock-blake3", "MLDSA65"] {
            let mut changed = original.clone();
            changed.sponsor.sponsor_key.algorithm = alias.into();
            assert!(verify_signed(&changed).is_err());
        }
    }

    #[test]
    fn resigning_preserves_intent_manifest_and_sponsor_authorization_ids() {
        let (inner, sponsor, original) = fixture(secondary_algorithm(), "mldsa65");
        let mut changed = original.clone();
        changed.recovery.signatures[0] =
            inner.inner(&changed.recovery.operation, SignatureRole::Operation);
        sponsor.sponsor(&mut changed);
        assert!(verify_signed(&changed).is_ok());
        assert_eq!(
            wire::operation_id(&original.recovery.operation).unwrap(),
            wire::operation_id(&changed.recovery.operation).unwrap()
        );
        assert_eq!(
            wire::signer_manifest_digest(&original.recovery).unwrap(),
            wire::signer_manifest_digest(&changed.recovery).unwrap()
        );
        assert_eq!(
            wire::authorization_id(&original.sponsor).unwrap(),
            wire::authorization_id(&changed.sponsor).unwrap()
        );
        // Randomized signatures identify different envelopes, never new intents.
        assert_ne!(
            wire::encode(&original).unwrap(),
            wire::encode(&changed).unwrap()
        );
        assert_ne!(
            wire::envelope_hash(&original).unwrap(),
            wire::envelope_hash(&changed).unwrap()
        );
    }

    #[test]
    fn bounded_noncanonical_and_duplicate_envelopes_reject_before_crypto() {
        let (_, _, original) = fixture("mldsa65", "mldsa65");
        let encoded = wire::encode(&original).unwrap();
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(verify_bytes(&trailing).is_err());
        assert!(verify_bytes(&encoded[..encoded.len() - 1]).is_err());
        assert!(verify_bytes(&vec![0; 262_145]).is_err());
        let mut duplicate = original.clone();
        duplicate
            .recovery
            .signatures
            .push(duplicate.recovery.signatures[0].clone());
        assert!(verify_signed(&duplicate).is_err());
        let mut wrong_length = original;
        wrong_length.signature.pop();
        assert!(verify_signed(&wrong_length).is_err());
    }
}


/// The same valid public development signature must fail in the strict backend.
#[test]
fn frozen_mldsa87_signature_matches_explicit_backend_selection() {
    let bytes = include_bytes!("fixtures/mldsa87/recovery_sponsor.bin");
    let value = wire::decode(bytes).unwrap();
    let result = verify_signed(&value);
    assert!(matches!(result, Err(dytallix_runtime_crypto::recovery_sponsor::SponsorVerificationError::UnsupportedAlgorithm)));
}
