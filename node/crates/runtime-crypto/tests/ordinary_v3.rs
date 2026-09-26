//! Signature tests for the provisional v3 codec. No consensus activation.

use dytallix_protocol_types::ordinary as v2;
use dytallix_protocol_types::{
    ordinary::Limits as V2Limits,
    ordinary_v3::{
        self as v3, Action, Denomination, OrdinaryTransaction, SignedOrdinary, V3Limits,
    },
    recovery::{KeyIdentity, RecoveryDomain},
};
use dytallix_runtime_crypto::ordinary_v3::verify_bytes;
use dytallix_runtime_crypto::ordinary_v3::verify_signed;
use std::collections::BTreeSet;

fn limits() -> V3Limits {
    V3Limits {
        ordinary: V2Limits {
            max_wire_bytes: 65_536,
            max_actions: 16,
            max_identifier_bytes: 64,
            max_data_bytes: 1024,
            max_memo_bytes: 128,
            max_consensus_key_bytes: 4096,
            max_proof_bytes: 8192,
            max_expiry_lifetime: 100,
            allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
        },
        max_governance_action_bytes: 128,
    }
}

fn body(key: KeyIdentity) -> OrdinaryTransaction {
    let action_data = vec![1, 2, 3, 4];
    OrdinaryTransaction {
        domain: RecoveryDomain {
            network: 1,
            chain_id: "v3-signature-fixture".into(),
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
        memo: String::new(),
        actions: vec![
            Action::GovernanceProposal {
                proposal_id: 23,
                action_class: 17,
                action_digest: v3::governance_action_digest(17, &action_data).unwrap(),
                action_data,
            },
            Action::GovernanceDeposit {
                proposal_id: 23,
                amount_udgt: 100,
            },
            Action::GovernanceVote {
                proposal_id: 23,
                choice: v3::VoteChoice::Yes,
            },
        ],
    }
}

mod real {
    use super::*;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes, Signer},
    };
    use rand_core::OsRng;

    fn fixture() -> SignedOrdinary {
        let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut OsRng).unwrap();
        let body = body(KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: public.into_bytes().to_vec(),
        });
        let signing = v3::signing_bytes(&body, &limits()).unwrap();
        let signature = secret.try_sign(&signing, &[]).unwrap().to_vec();
        SignedOrdinary { body, signature }
    }

    #[test]
    fn authenticates_complete_v3_body_and_rejects_v2_bytes() {
        let signed = fixture();
        let bytes = v3::encode(&signed, &limits()).unwrap();
        let verified = verify_bytes(&bytes, &limits()).unwrap();
        assert_eq!(verified, verify_signed(&signed, &limits()).unwrap());
        assert_eq!(verified.body(), &signed.body);
        assert_eq!(
            verified.transaction_id(),
            v3::transaction_id(&signed.body, &limits()).unwrap()
        );
        assert_eq!(
            verified.envelope_hash(),
            v3::envelope_hash(&signed, &limits()).unwrap()
        );
        assert!(v2::decode(&bytes, &limits().ordinary).is_err());
    }

    #[test]
    fn rejects_unsigned_governance_and_fee_changes() {
        let signed = fixture();
        let mut changed = signed.clone();
        changed.body.maximum_fee += 1;
        assert!(v3::encode(&changed, &limits()).is_ok());
        assert!(verify_signed(&changed, &limits()).is_err());

        let mut changed = signed.clone();
        changed.body.actions[2] = Action::GovernanceVote {
            proposal_id: 23,
            choice: v3::VoteChoice::NoWithVeto,
        };
        assert!(v3::encode(&changed, &limits()).is_ok());
        assert!(verify_signed(&changed, &limits()).is_err());

        let mut changed = signed;
        if let Action::GovernanceProposal {
            action_data,
            action_digest,
            ..
        } = &mut changed.body.actions[0]
        {
            action_data[0] ^= 1;
            *action_digest = v3::governance_action_digest(17, action_data).unwrap();
        }
        assert!(v3::encode(&changed, &limits()).is_ok());
        assert!(verify_signed(&changed, &limits()).is_err());
    }
}

