use super::*;
use crate::{
    addr::{AccountAddress, AddressNetwork},
    ordinary_meter::{LogicalField, LogicalValue, RecoveryCeilings, ResourceUsage},
    recovery_fees::RecoveryAccount,
};
use dytallix_protocol_types::{
    ordinary::{self, Limits},
    ordinary_fees::FeeProfile as OrdinaryFeeProfile,
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile as RecoveryFeeProfile,
};
use dytallix_runtime_crypto::ordinary_v3::verify_signed;
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use std::collections::{BTreeMap, BTreeSet};

fn profile() -> FeeProfileV3 {
    FeeProfileV3 {
        base: OrdinaryFeeProfile {
            ordinary_fee_contract_version: 1,
            version: 1,
            activation_height: 1,
            denomination: ordinary::Denomination::Udrt,
            gas_price: 2,
            minimum_gas: 1,
            max_transaction_gas: 1000,
            max_block_transaction_gas: 3000,
            max_block_transaction_bytes: 100_000,
            max_block_signature_checks: 10,
            max_fee_cap: 2000,
            limits: Limits {
                max_wire_bytes: 65_536,
                max_actions: 1,
                max_identifier_bytes: 64,
                max_data_bytes: 1024,
                max_memo_bytes: 128,
                max_consensus_key_bytes: 4096,
                max_proof_bytes: 8192,
                max_expiry_lifetime: 100,
                allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
            },
            transaction_overhead: 2,
            receipt_metadata_cost: 5,
            wire_byte_cost: 0,
            read_byte_cost: 1,
            write_byte_cost: 1,
            action_costs: [17; 12],
            signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
            validator_proof_profile_digest: [9; 32],
            validator_proof_costs: BTreeMap::from([("mldsa65".into(), 11)]),
        },
        version: 2,
        activation_height: 2,
        max_governance_action_bytes: 1024,
        governance_action_costs: [7, 11, 13],
    }
}

fn recovery_profile() -> RecoveryFeeProfile {
    RecoveryFeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 1,
        max_transaction_gas: 100_000,
        max_block_gas: 200_000,
        max_block_recovery_bytes: 100_000,
        max_block_recovery_signatures: 1,
        max_fee_cap: 100_000,
        max_pending_accounts: 1,
        max_due_expiry_events_per_height: 1,
        mandatory_expiry_gas_budget: 1,
        expiry_event_gas_cost: 1,
        action_costs: [1; 9],
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
    }
}

struct Fixture {
    secret: ml_dsa_65::PrivateKey,
    signed: SignedOrdinary,
    verified: VerifiedOrdinaryV3,
    profile: FeeProfileV3,
    book: RecoveryBook,
    financial: FinancialState,
}

impl Fixture {
    fn new() -> Self {
        let profile = profile();
        let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let key = KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: public.into_bytes().to_vec(),
        };
        let actor = [3; 32];
        let domain = RecoveryDomain {
            network: 3,
            chain_id: "v3-fee-test".into(),
            genesis_digest: [7; 32],
            account_id: actor,
        };
        let recovery = RecoveryState::new(
            domain.clone(),
            RecoveryConfig {
                timing_version: 1,
                recovery_delay: 2,
                finalization_window: 3,
                policy_delay: 2,
                policy_window: 3,
                submission_lifetime: 100,
                algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
            },
            key.clone(),
            0,
        )
        .unwrap();
        let address = AccountAddress::from_account_id(AddressNetwork::Development, actor).encode();
        let mut book = RecoveryBook::new(
            recovery_profile(),
            vec![RecoveryAccount {
                address: address.clone(),
                recovery,
                sponsor_nonce: 0,
            }],
        )
        .unwrap();
        book.accounts.get_mut(&hex::encode(actor)).unwrap().recovery = book.accounts
            [&hex::encode(actor)]
            .recovery
            .advance_height(1)
            .unwrap();
        book.last_height = 1;
        book.validate().unwrap();
        book = book.begin_block(2).unwrap().book;
        assert_eq!(book.last_height, 2);
        let action = v3::Action::GovernanceVote {
            proposal_id: 1,
            choice: v3::VoteChoice::Yes,
        };
        let mut signed = SignedOrdinary {
            body: v3::OrdinaryTransaction {
                domain,
                authorization_generation: 0,
                spending_nonce: 0,
                key,
                expiry_height: 20,
                ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
                fee_profile_version: profile.version,
                fee_profile_digest: ordinary_fees_v3::profile_digest(&profile).unwrap(),
                fee_denomination: v3::Denomination::Udrt,
                maximum_fee: 1000,
                gas_limit: 500,
                memo: String::new(),
                actions: vec![action],
            },
            signature: Vec::new(),
        };
        let bytes = v3::signing_bytes(&signed.body, &profile.limits()).unwrap();
        signed.signature = secret.try_sign(&bytes, &[]).unwrap().to_vec();
        let verified = verify_signed(&signed, &profile.limits()).unwrap();
        let fee_asset = Asset {
            owner: actor,
            denomination: Denomination::Udrt,
        };
        let financial = FinancialState {
            balances: BTreeMap::from([(fee_asset, 10_000)]),
            eligible: BTreeMap::from([(fee_asset, 10_000)]),
            native_nonces: BTreeMap::from([(address, 0)]),
            withheld_udrt: 0,
        };
        Self {
            secret,
            signed,
            verified,
            profile,
            book,
            financial,
        }
    }

    fn resign(&mut self) {
        self.signed.body.fee_profile_digest =
            ordinary_fees_v3::profile_digest(&self.profile).unwrap();
        self.signed.body.fee_profile_version = self.profile.version;
        let bytes = v3::signing_bytes(&self.signed.body, &self.profile.limits()).unwrap();
        self.signed.signature = self.secret.try_sign(&bytes, &[]).unwrap().to_vec();
        self.verified = verify_signed(&self.signed, &self.profile.limits()).unwrap();
    }

    fn block(&self) -> SharedBlockMeter {
        SharedBlockMeter::new(
            &self.profile.base,
            RecoveryCeilings {
                max_gas: 3000,
                max_bytes: 100_000,
                max_signatures: 10,
                mandatory_expiry_gas: 1,
            },
        )
        .unwrap()
    }

    fn plan(&self, work: &ActionWork, block: &mut SharedBlockMeter) -> Result<FeePlan> {
        plan_governance_fee(
            &self.signed,
            &self.verified,
            &self.profile,
            &self.book,
            &self.financial,
            2,
            7,
            work,
            block,
        )
    }
}

fn empty_work(end: ActionEnd) -> ActionWork {
    ActionWork {
        validation_reads: Vec::new(),
        action_reads: Vec::new(),
        proposed_writes: Vec::new(),
        end,
    }
}

#[test]
fn success_reserves_full_cap_charges_measured_work_and_advances_both_nonces() {
    let f = Fixture::new();
    let before = f.financial.clone();
    let book_before = f.book.clone();
    let mut block = f.block();
    let plan = f.plan(&empty_work(ActionEnd::Applied), &mut block).unwrap();
    assert_eq!(plan.reservation.cap, 1000);
    assert_eq!(plan.reservation.balance_while_reserved, 9000);
    assert_eq!(plan.receipt.gas_used, 2 + 3 + 5 + 13);
    assert_eq!(plan.receipt.charge, 46);
    assert_eq!(plan.receipt.released_cap, 954);
    assert_eq!(plan.fee_checkpoint.balances[&plan.reservation.asset], 9954);
    assert_eq!(plan.fee_checkpoint.eligible[&plan.reservation.asset], 9954);
    assert_eq!(plan.fee_checkpoint.withheld_udrt, 46);
    assert_eq!(plan.receipt.nonce_before, 0);
    assert_eq!(plan.receipt.nonce_after, 1);
    assert_eq!(
        plan.authority_checkpoint.accounts[&hex::encode([3; 32])]
            .recovery
            .spending_nonce,
        1
    );
    assert_eq!(
        plan.fee_checkpoint.native_nonces.values().next().copied(),
        Some(1)
    );
    assert_eq!(plan.receipt.transaction_id, f.verified.transaction_id());
    assert_eq!(plan.receipt.envelope_hash, f.verified.envelope_hash());
    assert_eq!(
        plan.receipt.profile_digest,
        ordinary_fees_v3::profile_digest(&f.profile).unwrap()
    );
    assert_eq!(
        (plan.receipt.block_height, plan.receipt.block_index),
        (2, 7)
    );
    assert_eq!(plan.receipt.contract_version, ORDINARY_FEE_CONTRACT_VERSION);
    assert_eq!(plan.receipt.outcome, Outcome::Success);
    assert_eq!(plan.disposition, ActionDisposition::KeepProposed);
    assert_eq!(block.usage().gas, plan.receipt.gas_used);
    assert_eq!(f.financial, before);
    assert_eq!(f.book, book_before);
}

#[test]
fn application_failure_keeps_fee_and_nonce_but_discards_proposed_effects() {
    let f = Fixture::new();
    let write = LogicalRecord::new(
        b"governance:vote:1",
        &[LogicalField {
            id: 1,
            value: LogicalValue::U64(1),
        }],
        4096,
    )
    .unwrap();
    let mut work = empty_work(ActionEnd::ApplicationFailure);
    work.proposed_writes.push(write);
    let mut block = f.block();
    let plan = f.plan(&work, &mut block).unwrap();
    assert_eq!(plan.receipt.outcome, Outcome::ApplicationFailure);
    assert_eq!(plan.disposition, ActionDisposition::DiscardAll);
    assert!(plan.receipt.gas_used > 23);
    assert_eq!(plan.receipt.charge, u128::from(plan.receipt.gas_used) * 2);
    assert_eq!(plan.receipt.nonce_after, 1);
    assert_eq!(plan.fee_checkpoint.withheld_udrt, plan.receipt.charge);
}

#[test]
fn out_of_gas_keeps_exact_limit_fee_and_nonce_without_action_effects() {
    let mut f = Fixture::new();
    f.signed.body.gas_limit = 10;
    f.resign();
    let mut block = f.block();
    let plan = f.plan(&empty_work(ActionEnd::Applied), &mut block).unwrap();
    assert_eq!(plan.receipt.outcome, Outcome::OutOfGas);
    assert_eq!(plan.receipt.gas_used, 10);
    assert_eq!(plan.receipt.charge, 20);
    assert_eq!(plan.disposition, ActionDisposition::DiscardAll);
    assert_eq!(plan.receipt.nonce_after, 1);
}

#[test]
fn minimum_gas_applies_to_charge_without_forging_measured_usage() {
    let mut f = Fixture::new();
    f.profile.base.minimum_gas = 30;
    f.resign();
    let mut block = f.block();
    let plan = f.plan(&empty_work(ActionEnd::Applied), &mut block).unwrap();
    assert_eq!(plan.receipt.gas_used, 23);
    assert_eq!(plan.receipt.charge, 60);
    assert_eq!(block.usage().gas, 23);
}

#[test]
fn insufficient_full_cap_and_stale_nonce_reject_before_meter_use() {
    let mut f = Fixture::new();
    f.financial.eligible.values_mut().for_each(|v| *v = 999);
    let mut block = f.block();
    assert!(matches!(
        f.plan(&empty_work(ActionEnd::Applied), &mut block),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(block.usage(), ResourceUsage::default());
    f.financial.eligible.values_mut().for_each(|v| *v = 10_000);
    f.book
        .accounts
        .get_mut(&hex::encode([3; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 1;
    f.financial.native_nonces.values_mut().for_each(|v| *v = 1);
    assert!(matches!(
        f.plan(&empty_work(ActionEnd::Applied), &mut block),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(block.usage(), ResourceUsage::default());
}

#[test]
fn signature_proof_must_bind_same_metered_envelope() {
    let mut f = Fixture::new();
    let verified = f.verified.clone();
    f.signed.signature[0] ^= 1;
    let mut block = f.block();
    assert!(matches!(
        plan_governance_fee(
            &f.signed,
            &verified,
            &f.profile,
            &f.book,
            &f.financial,
            2,
            7,
            &empty_work(ActionEnd::Applied),
            &mut block
        ),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(block.usage(), ResourceUsage::default());
}

#[test]
fn committed_parent_book_cannot_replace_staged_block_book() {
    let mut f = Fixture::new();
    f.book.last_height = 1;
    f.book
        .accounts
        .get_mut(&hex::encode([3; 32]))
        .unwrap()
        .recovery
        .last_height = 1;
    f.book.validate().unwrap();
    let mut block = f.block();
    assert!(matches!(
        f.plan(&empty_work(ActionEnd::Applied), &mut block),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(block.usage(), ResourceUsage::default());
}
