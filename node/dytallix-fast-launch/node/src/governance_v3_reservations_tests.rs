use super::*;
use crate::ordinary_reservations::{
    Eligibility, QueueLimits, ReservationError, ReservationLedger, ReservationStatus,
};
use dytallix_protocol_types::{
    ordinary::{self, Limits},
    ordinary_fees::FeeProfile,
    ordinary_fees_v3::ORDINARY_FEE_CONTRACT_VERSION,
    recovery::{KeyIdentity, RecoveryDomain},
};
use dytallix_runtime_crypto::ordinary_v3::verify_signed;
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use std::collections::{BTreeMap, BTreeSet};

const CONTEXT: [u8; 32] = [7; 32];
const ACTOR: [u8; 32] = [8; 32];

fn profile() -> FeeProfileV3 {
    FeeProfileV3 {
        base: FeeProfile {
            ordinary_fee_contract_version: 1,
            version: 1,
            activation_height: 1,
            denomination: ordinary::Denomination::Udrt,
            gas_price: 1,
            minimum_gas: 1,
            max_transaction_gas: 1000,
            max_block_transaction_gas: 2000,
            max_block_transaction_bytes: 65_536,
            max_block_signature_checks: 4,
            max_fee_cap: 1000,
            limits: Limits {
                max_wire_bytes: 65_536,
                max_actions: 2,
                max_identifier_bytes: 64,
                max_data_bytes: 1024,
                max_memo_bytes: 128,
                max_consensus_key_bytes: 4096,
                max_proof_bytes: 8192,
                max_expiry_lifetime: 100,
                allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
            },
            transaction_overhead: 1,
            receipt_metadata_cost: 1,
            wire_byte_cost: 1,
            read_byte_cost: 1,
            write_byte_cost: 1,
            action_costs: [1; 12],
            signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
            validator_proof_profile_digest: [9; 32],
            validator_proof_costs: BTreeMap::from([("mldsa65".into(), 1)]),
        },
        version: 2,
        activation_height: 2,
        max_governance_action_bytes: 1024,
        governance_action_costs: [1; 3],
    }
}

fn signed_actions(profile: &FeeProfileV3, actions: Vec<Action>, nonce: u64) -> SignedOrdinary {
    let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
    let mut signed = SignedOrdinary {
        body: v3::OrdinaryTransaction {
            domain: RecoveryDomain {
                network: 3,
                chain_id: "v3-reservation-test".into(),
                genesis_digest: [3; 32],
                account_id: ACTOR,
            },
            authorization_generation: 0,
            spending_nonce: nonce,
            key: KeyIdentity {
                algorithm: "mldsa65".into(),
                public_key: public.into_bytes().to_vec(),
            },
            expiry_height: 20,
            ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
            fee_profile_version: profile.version,
            fee_profile_digest: ordinary_fees_v3::profile_digest(profile).unwrap(),
            fee_denomination: v3::Denomination::Udrt,
            maximum_fee: 1000,
            gas_limit: 1000,
            memo: String::new(),
            actions,
        },
        signature: vec![],
    };
    let message = v3::signing_bytes(&signed.body, &profile.limits()).unwrap();
    signed.signature = secret.try_sign(&message, &[]).unwrap().to_vec();
    signed
}

fn signed(profile: &FeeProfileV3, action: Action, nonce: u64) -> SignedOrdinary {
    signed_actions(profile, vec![action], nonce)
}

fn funds(udrt: u128, dgt_total: u128, dgt_unrestricted: u128) -> Eligibility {
    let fee = Asset {
        owner: ACTOR,
        denomination: Denomination::Udrt,
    };
    let deposit = Asset {
        owner: ACTOR,
        denomination: Denomination::Udgt,
    };
    Eligibility {
        total: BTreeMap::from([(fee, udrt), (deposit, dgt_total)]),
        unrestricted: BTreeMap::from([(fee, udrt), (deposit, dgt_unrestricted)]),
    }
}

fn ledger() -> ReservationLedger {
    ReservationLedger::new(
        CONTEXT,
        QueueLimits {
            max_entries: 4,
            max_wire_bytes: 200_000,
            max_signature_work: 4,
            max_action_debits_per_entry: 2,
        },
    )
    .unwrap()
}

fn request(profile: &FeeProfileV3, signed: &SignedOrdinary) -> ReservationRequest {
    let verified = verify_signed(signed, &profile.limits()).unwrap();
    reservation_request(signed, &verified, profile, CONTEXT, 2).unwrap()
}

#[test]
fn deposit_reserves_full_fee_and_unrestricted_dgt() {
    let profile = profile();
    let signed = signed(
        &profile,
        Action::GovernanceDeposit {
            proposal_id: 1,
            amount_udgt: 17,
        },
        4,
    );
    let request = request(&profile, &signed);
    assert_eq!(request.context_digest, CONTEXT);
    assert_eq!(request.payer, ACTOR);
    assert_eq!(request.nonce, 4);
    assert_eq!(request.fee_cap_udrt, 1000);
    assert_eq!(request.signature_work, 1);
    assert_eq!(
        request.wire_bytes,
        u64::try_from(v3::encode(&signed, &profile.limits()).unwrap().len()).unwrap()
    );
    assert_eq!(request.action_debits, request.unrestricted_debits);
    assert_eq!(request.action_debits.len(), 1);
    assert_eq!(request.action_debits[0].amount, 17);
    assert_eq!(request.action_debits[0].kind, DebitKind::Outflow);
    assert_eq!(
        request.id,
        ReservationId::OrdinaryV3 {
            transaction_id: v3::transaction_id(&signed.body, &profile.limits()).unwrap(),
            profile_digest: ordinary_fees_v3::profile_digest(&profile).unwrap(),
        }
    );
    let mut ledger = ledger();
    for (liquidity, error) in [
        (
            funds(999, 100, 100),
            ReservationError::InsufficientLiquidity,
        ),
        (
            funds(1000, 100, 16),
            ReservationError::InsufficientLiquidity,
        ),
    ] {
        let before = ledger.clone();
        assert_eq!(ledger.reserve(&request, &liquidity), Err(error));
        assert_eq!(ledger, before);
    }
    assert_eq!(
        ledger.reserve(&request, &funds(1000, 100, 17)),
        Ok(ReservationStatus::Added)
    );
    assert_eq!(
        ledger
            .reserved(Asset {
                owner: ACTOR,
                denomination: Denomination::Udrt,
            })
            .unwrap()
            .fee,
        1000
    );
    assert_eq!(
        ledger
            .reserved_unrestricted(Asset {
                owner: ACTOR,
                denomination: Denomination::Udgt,
            })
            .unwrap()
            .action,
        17
    );
}

#[test]
fn proposal_and_vote_reserve_no_dgt() {
    let profile = profile();
    let data = vec![1, 2, 3];
    for action in [
        Action::GovernanceProposal {
            proposal_id: 1,
            action_class: 9,
            action_digest: v3::governance_action_digest(9, &data).unwrap(),
            action_data: data,
        },
        Action::GovernanceVote {
            proposal_id: 1,
            choice: v3::VoteChoice::Yes,
        },
    ] {
        let signed = signed(&profile, action, 0);
        let request = request(&profile, &signed);
        assert!(request.action_debits.is_empty());
        assert!(request.unrestricted_debits.is_empty());
        assert_eq!(
            ledger().reserve(&request, &funds(1000, 0, 0)),
            Ok(ReservationStatus::Added)
        );
    }
}

#[test]
fn verified_binding_rejects_changed_envelope_and_profile() {
    let profile = profile();
    let signed = signed(
        &profile,
        Action::GovernanceVote {
            proposal_id: 1,
            choice: v3::VoteChoice::No,
        },
        0,
    );
    let verified = verify_signed(&signed, &profile.limits()).unwrap();
    let mut changed = signed.clone();
    changed.signature[0] ^= 1;
    assert!(reservation_request(&changed, &verified, &profile, CONTEXT, 2).is_err());
    changed = signed.clone();
    changed.body.maximum_fee -= 1;
    assert!(reservation_request(&changed, &verified, &profile, CONTEXT, 2).is_err());
    let mut changed_profile = profile.clone();
    changed_profile.base.gas_price = 2;
    assert!(reservation_request(&signed, &verified, &changed_profile, CONTEXT, 2).is_err());
    assert!(reservation_request(&signed, &verified, &profile, CONTEXT, 1).is_err());
}

#[test]
fn zero_deposit_and_multiple_actions_cannot_make_reservations() {
    let profile = profile();
    let zero = signed(
        &profile,
        Action::GovernanceDeposit {
            proposal_id: 1,
            amount_udgt: 0,
        },
        0,
    );
    let verified = verify_signed(&zero, &profile.limits()).unwrap();
    assert!(reservation_request(&zero, &verified, &profile, CONTEXT, 2).is_err());
    let multiple = signed_actions(
        &profile,
        vec![
            Action::GovernanceVote {
                proposal_id: 1,
                choice: v3::VoteChoice::Yes,
            },
            Action::GovernanceVote {
                proposal_id: 1,
                choice: v3::VoteChoice::No,
            },
        ],
        0,
    );
    let verified = verify_signed(&multiple, &profile.limits()).unwrap();
    assert!(reservation_request(&multiple, &verified, &profile, CONTEXT, 2).is_err());
}
