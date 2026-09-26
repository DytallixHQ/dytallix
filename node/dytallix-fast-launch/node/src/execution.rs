//! Transaction settlement for the selected node.
//! Message effects roll back to the accepted fee-and-nonce checkpoint on failure.
//! This module does not provide atomic block settlement or complete supply accounting.

use crate::gas::{intrinsic_gas, Gas, GasError, GasMeter, GasSchedule, TxKind};
use crate::runtime::fee_burn::FeeBurnEngine;
use crate::settlement::{self, Settlement};
use crate::state::State;
use crate::storage::receipts::{TxReceipt, TxStatus, RECEIPT_FORMAT_VERSION};
use crate::storage::tx::Transaction;
use crate::storage::tx::TxMessage;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: u128, available: u128 },

    #[error("InvalidNonce: expected {expected}, got {actual}")]
    InvalidNonce { expected: u64, actual: u64 },

    #[error("Gas error: {0}")]
    Gas(#[from] GasError),

    #[error("Overflow in fee calculation")]
    FeeOverflow,

    #[error("State error: {0}")]
    State(String),
}

/// Execution context for a single transaction
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub gas_limit: Gas,
    pub gas_price: u64,
    pub gas_meter: GasMeter,
    pub state_changes: Vec<StateChange>,
    pub events: Vec<String>, // Simplified events for now
}

/// Represents a state change that can be reverted
#[derive(Debug, Clone)]
pub struct StateChange {
    pub address: String,
    pub denom: String,
    pub old_balance: u128,
    pub new_balance: u128,
}

/// Result of transaction execution
#[derive(Debug)]
pub struct ExecutionResult {
    pub receipt: TxReceipt,
    pub state_changes: Vec<StateChange>,
    pub gas_used: Gas,
    pub success: bool,
}

impl ExecutionContext {
    pub fn new(gas_limit: Gas, gas_price: u64) -> Self {
        Self {
            gas_limit,
            gas_price,
            gas_meter: GasMeter::new(gas_limit),
            state_changes: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Calculate upfront fee with overflow protection
    pub fn calculate_upfront_fee(&self) -> Result<u128, ExecutionError> {
        let gas_limit = self.gas_limit as u128;
        let gas_price = self.gas_price as u128;

        gas_limit
            .checked_mul(gas_price)
            .ok_or(ExecutionError::FeeOverflow)
    }

    /// Record a state change for potential revert
    pub fn record_state_change(
        &mut self,
        address: String,
        denom: String,
        old_balance: u128,
        new_balance: u128,
    ) {
        self.state_changes.push(StateChange {
            address,
            denom,
            old_balance,
            new_balance,
        });
    }

    /// Consume gas and check if we're out of gas
    pub fn consume_gas(&mut self, amount: Gas, operation: &str) -> Result<(), GasError> {
        self.gas_meter.consume(amount, operation)
    }

    /// Get gas used so far
    pub fn gas_used(&self) -> Gas {
        self.gas_meter.gas_used()
    }
}

/// Commit one transaction's account effects, fee record, switch records, and receipt.
/// An outer error means storage or settlement integrity failed. The producer must stop.
/// A returned failed receipt is a normal rejected or charged-but-reverted transaction.
pub fn execute_transaction(
    tx: &Transaction,
    state: &mut State,
    block_height: u64,
    tx_index: u32,
    gas_schedule: &GasSchedule,
    fee_burn_engine: Option<&mut FeeBurnEngine>,
) -> anyhow::Result<ExecutionResult> {
    anyhow::ensure!(
        !crate::signed_transaction::contains_reward_messages(tx),
        "Reward messages require signed block settlement"
    );
    let storage = state.storage.clone();
    let _guard = storage.lock_execution()?;
    crate::genesis::reject_qualified_state(&storage)?;
    anyhow::ensure!(
        storage.db.get("rewards:v2:state")?.is_none(),
        "Reward mode requires signed block settlement"
    );
    anyhow::ensure!(
        storage.db.get("execution:block:v1:head")?.is_none(),
        "Use block settlement for this database"
    );
    if let Some(receipt) = settlement::existing(&storage, tx, block_height, tx_index)? {
        // A matching retry must not charge again or update diagnostic burn totals.
        state.accounts.clear();
        return Ok(ExecutionResult {
            success: receipt.success,
            gas_used: receipt.gas_used,
            receipt,
            state_changes: Vec::new(),
        });
    }
    let mut staged = Settlement::new(storage.clone());
    let result = stage_transaction(tx, &mut staged, block_height, tx_index, gas_schedule)?;
    if !result.accepted {
        return Ok(result.result);
    }
    let success = result.result.success;
    let receipt = &result.result.receipt;
    let upfront = u128::from(receipt.gas_limit) * u128::from(receipt.gas_price);
    // Burn tracking remains a legacy diagnostic, not a supply mutation. Prepare it
    // separately so a failed database commit cannot publish diagnostic success.
    let mut next_burn = fee_burn_engine.as_deref().cloned();
    if success {
        if let Some(engine) = next_burn.as_mut() {
            if let Err(error) =
                engine.process_fee_burn(tx.hash.clone(), block_height, upfront, state)
            {
                tracing::warn!(%error,"Fee burn diagnostic rejected");
                next_burn = None;
            }
        }
    }
    staged.commit(state, tx, receipt)?;
    if success {
        if let (Some(engine), Some(next)) = (fee_burn_engine, next_burn) {
            *engine = next;
        }
    }
    Ok(result.result)
}

pub(crate) struct StagedExecution {
    pub result: ExecutionResult,
    pub accepted: bool,
}
pub(crate) fn stage_transaction(
    tx: &Transaction,
    staged: &mut Settlement,
    block_height: u64,
    tx_index: u32,
    gas_schedule: &GasSchedule,
) -> anyhow::Result<StagedExecution> {
    let nonce = staged.account(&tx.from)?.nonce;
    let rejected = |error: String, limit, price| StagedExecution {
        accepted: false,
        result: ExecutionResult {
            receipt: create_failed_receipt(tx, 0, limit, price, error, block_height, tx_index),
            state_changes: Vec::new(),
            gas_used: 0,
            success: false,
        },
    };
    if tx.nonce != nonce {
        return Ok(rejected(
            ExecutionError::InvalidNonce {
                expected: nonce,
                actual: tx.nonce,
            }
            .to_string(),
            0,
            0,
        ));
    }
    if nonce == u64::MAX {
        return Ok(rejected("Account nonce exhausted".into(), 0, 0));
    }
    if let Some(messages) = &tx.messages {
        for message in messages {
            let from = match message {
                TxMessage::Send { from, .. }
                | TxMessage::Data { from, .. }
                | TxMessage::DmsRegister { from, .. }
                | TxMessage::DmsPing { from, .. }
                | TxMessage::DmsClaim { from, .. }
                | TxMessage::RewardBond { from, .. }
                | TxMessage::RewardBeginUnbond { from, .. }
                | TxMessage::RewardClaim { from }
                | TxMessage::ValidatorRegister { from, .. }
                | TxMessage::ValidatorRotateKey { from, .. }
                | TxMessage::ValidatorExit { from, .. }
                | TxMessage::ValidatorWithdraw { from, .. } => from,
            };
            if from != &tx.from {
                return Ok(rejected(
                    "Message sender differs from transaction sender".into(),
                    0,
                    0,
                ));
            }
        }
    }
    let (gas_limit, gas_price) = match crate::transaction_cost::effective_gas(tx) {
        Ok(gas) => gas,
        Err(error) => return Ok(rejected(error, 0, 0)),
    };
    let mut ctx = ExecutionContext::new(gas_limit, gas_price);
    let upfront = ctx.calculate_upfront_fee()?;
    let available = staged.account(&tx.from)?.balance_of("udrt");
    if available < upfront {
        return Ok(rejected(
            format!("InsufficientFunds: required {upfront}, available {available}"),
            gas_limit,
            gas_price,
        ));
    }
    staged.charge(&tx.from, upfront)?;
    let checkpoint = staged.clone();
    let messages = match &tx.messages {
        Some(messages) => messages.clone(),
        None => vec![TxMessage::Send {
            from: tx.from.clone(),
            to: tx.to.clone(),
            denom: tx.denom.clone(),
            amount: tx.amount,
        }],
    };
    let execution: anyhow::Result<()> = (|| {
        ctx.consume_gas(1, "tx_overhead")?;
        let size = tx
            .hash
            .len()
            .checked_add(tx.from.len())
            .and_then(|n| n.checked_add(tx.to.len()))
            .and_then(|n| n.checked_add(64))
            .ok_or_else(|| settlement::RuleViolation("Transaction size exceeds usize".into()))?;
        let intrinsic = intrinsic_gas(&TxKind::Transfer, size, messages.len(), gas_schedule)?;
        ctx.consume_gas(intrinsic, "intrinsic")?;
        for (index, message) in messages.iter().enumerate() {
            execute_message(message, staged, &mut ctx, block_height, tx.nonce)
                .map_err(|error| error.context(format!("Execution failed at message {index}")))?;
        }
        Ok(())
    })();
    let (success, error) = match execution {
        Ok(()) => (true, None),
        Err(error) if error.is::<GasError>() || error.is::<settlement::RuleViolation>() => {
            *staged = checkpoint;
            ctx.state_changes.clear();
            let detail = if matches!(
                error.downcast_ref::<GasError>(),
                Some(GasError::OutOfGas { .. })
            ) {
                if tx.messages.is_none() || error.chain().count() == 1 {
                    "OutOfGas".into()
                } else {
                    format!("{error:#}")
                }
            } else {
                format!("{error:#}")
            };
            (false, Some(detail))
        }
        Err(error) => return Err(error),
    };
    let receipt = if success {
        create_success_receipt(
            tx,
            ctx.gas_used(),
            gas_limit,
            gas_price,
            block_height,
            tx_index,
        )
    } else {
        create_failed_receipt(
            tx,
            ctx.gas_used(),
            gas_limit,
            gas_price,
            error.expect("failed execution has an error"),
            block_height,
            tx_index,
        )
    };
    Ok(StagedExecution {
        accepted: true,
        result: ExecutionResult {
            receipt,
            state_changes: ctx.state_changes,
            gas_used: ctx.gas_meter.gas_used(),
            success,
        },
    })
}

// Local qualification cost. Production verification costs require calibration.
fn consume_validator_key_gas(
    ctx: &mut ExecutionContext,
    key: &str,
    proof: &str,
) -> anyhow::Result<()> {
    let bytes = key
        .len()
        .checked_add(proof.len())
        .and_then(|n| u64::try_from(n).ok())
        .ok_or_else(|| settlement::RuleViolation("Validator proof size exceeds u64".into()))?;
    let gas = bytes
        .checked_add(10_000)
        .ok_or_else(|| settlement::RuleViolation("Validator gas exceeds u64".into()))?;
    ctx.consume_gas(gas, "validator_key_proof")?;
    Ok(())
}

fn execute_message(
    message: &TxMessage,
    staged: &mut Settlement,
    ctx: &mut ExecutionContext,
    height: u64,
    nonce: u64,
) -> anyhow::Result<()> {
    match message {
        TxMessage::Send {
            from,
            to,
            denom,
            amount,
        } => {
            for (gas, operation) in [
                (40, "kv_read_from"),
                (40, "kv_read_to"),
                (120, "kv_write_from"),
                (120, "kv_write_to"),
            ] {
                ctx.consume_gas(gas, operation)?;
            }
            transfer(staged, ctx, from, to, denom, *amount)?;
        }
        TxMessage::RewardBond {
            from,
            validator,
            amount_udgt,
        } => {
            consume_reward_gas(ctx)?;
            staged.reward_bond(from, validator, *amount_udgt)?;
        }
        TxMessage::RewardBeginUnbond {
            from,
            validator,
            amount_udgt,
        } => {
            consume_reward_gas(ctx)?;
            staged.reward_begin_unbond(from, validator, *amount_udgt)?;
        }
        TxMessage::RewardClaim { from } => {
            consume_reward_gas(ctx)?;
            staged.reward_claim(from)?;
        }
        TxMessage::ValidatorRegister {
            from,
            validator,
            consensus_pubkey,
            proof,
            expires_at_height,
            amount_udgt,
        } => {
            consume_validator_key_gas(ctx, consensus_pubkey, proof)?;
            staged.validator_register(
                from,
                nonce,
                validator,
                consensus_pubkey,
                proof,
                *expires_at_height,
                *amount_udgt,
            )?;
        }
        TxMessage::ValidatorRotateKey {
            from,
            validator,
            consensus_pubkey,
            proof,
            expires_at_height,
        } => {
            consume_validator_key_gas(ctx, consensus_pubkey, proof)?;
            staged.validator_rotate_key(
                from,
                nonce,
                validator,
                consensus_pubkey,
                proof,
                *expires_at_height,
            )?;
        }
        TxMessage::ValidatorExit { from, validator } => {
            consume_reward_gas(ctx)?;
            staged.validator_exit(from, nonce, validator)?;
        }
        TxMessage::ValidatorWithdraw { from, unbond_id } => {
            consume_reward_gas(ctx)?;
            staged.validator_withdraw(from, unbond_id)?;
        }
        TxMessage::Data { data, .. } => ctx.consume_gas(data.len() as u64, "data_storage")?,
        TxMessage::DmsRegister {
            from,
            beneficiary,
            period,
        } => {
            ctx.consume_gas(1000, "dms_register")?;
            let period = u64::try_from(*period).map_err(|_| {
                settlement::RuleViolation("Inactivity-switch period exceeds u64".into())
            })?;
            staged.register(from, beneficiary, period, height)?;
        }
        TxMessage::DmsPing { from } => {
            ctx.consume_gas(500, "dms_ping")?;
            staged.ping(from, height)?;
        }
        TxMessage::DmsClaim { from, owner } => {
            ctx.consume_gas(2000, "dms_claim")?;
            let beneficiary = staged.beneficiary(owner, from, height)?;
            let balances = staged.account(owner)?.balances.clone();
            for (denom, amount) in balances {
                if amount > 0 {
                    transfer(staged, ctx, owner, &beneficiary, &denom, amount)?;
                }
            }
        }
    }
    Ok(())
}
// Use the existing two-read/two-write transfer cost class. These are selected-node
// execution inputs, not a separate reward burn or a new mainnet fee policy.
fn consume_reward_gas(ctx: &mut ExecutionContext) -> Result<(), GasError> {
    for (gas, operation) in [
        (40, "reward_read_account"),
        (40, "reward_read_state"),
        (120, "reward_write_account"),
        (120, "reward_write_state"),
    ] {
        ctx.consume_gas(gas, operation)?;
    }
    Ok(())
}

fn transfer(
    staged: &mut Settlement,
    ctx: &mut ExecutionContext,
    from: &str,
    to: &str,
    denom: &str,
    amount: u128,
) -> anyhow::Result<()> {
    let (old_from, new_from, old_to, new_to) = staged.transfer(from, to, denom, amount)?;
    if from != to {
        ctx.record_state_change(from.into(), denom.into(), old_from, new_from);
        ctx.record_state_change(to.into(), denom.into(), old_to, new_to);
    }
    Ok(())
}
/// Create a success receipt
fn create_success_receipt(
    tx: &Transaction,
    gas_used: Gas,
    gas_limit: Gas,
    gas_price: u64,
    block_height: u64,
    index: u32,
) -> TxReceipt {
    TxReceipt {
        receipt_version: RECEIPT_FORMAT_VERSION,
        tx_hash: tx.hash.clone(),
        status: TxStatus::Success,
        block_height: Some(block_height),
        index: Some(index),
        from: tx.from.clone(),
        to: tx.to.clone(),
        amount: tx.amount,
        fee: tx.fee,
        nonce: tx.nonce,
        error: None,
        gas_used,
        gas_limit,
        gas_price,
        gas_refund: 0, // Always 0 as per spec
        success: true,
    }
}

/// Create a failed receipt
fn create_failed_receipt(
    tx: &Transaction,
    gas_used: Gas,
    gas_limit: Gas,
    gas_price: u64,
    error: String,
    block_height: u64,
    index: u32,
) -> TxReceipt {
    TxReceipt {
        receipt_version: RECEIPT_FORMAT_VERSION,
        tx_hash: tx.hash.clone(),
        status: TxStatus::Failed,
        block_height: Some(block_height),
        index: Some(index),
        from: tx.from.clone(),
        to: tx.to.clone(),
        amount: tx.amount,
        fee: tx.fee,
        nonce: tx.nonce,
        error: Some(error),
        gas_used,
        gas_limit,
        gas_price,
        gas_refund: 0, // Always 0 as per spec
        success: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::state::Storage;
    use std::sync::Arc;

    fn create_test_state() -> (State, tempfile::TempDir) {
        // Use a unique temporary directory per test to avoid cross-test contamination
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Arc::new(Storage::open(dir.path().join("state.db")).unwrap());
        (State::new(storage), dir)
    }

    #[test]
    fn reward_messages_reject_standalone_execution_before_fees() {
        let (mut state, _dir) = create_test_state();
        state.set_balance("owner", "udrt", 1_000_000);
        for message in [
            TxMessage::RewardBond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 1,
            },
            TxMessage::RewardBeginUnbond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 1,
            },
            TxMessage::RewardClaim {
                from: "owner".into(),
            },
        ] {
            let tx = Transaction::base("standalone-reward", "owner", "owner", 0, 21_000, 0)
                .with_messages(vec![message]);
            let error = execute_transaction(&tx, &mut state, 1, 0, &GasSchedule::default(), None)
                .unwrap_err();
            assert!(error.to_string().contains("signed block settlement"));
            assert_eq!(state.balance_of("owner", "udrt"), 1_000_000);
            assert_eq!(state.nonce_of("owner"), 0);
        }
    }

    #[test]
    fn reward_state_disables_all_standalone_execution_before_first_block() {
        let (mut state, _dir) = create_test_state();
        state.set_balance("owner", "udrt", 1_000_000);
        state
            .storage
            .db
            .put("rewards:v2:state", b"state-present")
            .unwrap();
        let tx = Transaction::base("standalone-send", "owner", "receiver", 1, 21_000, 0);
        let error =
            execute_transaction(&tx, &mut state, 1, 0, &GasSchedule::default(), None).unwrap_err();
        assert!(error
            .to_string()
            .contains("Reward mode requires signed block settlement"));
        assert_eq!(state.balance_of("owner", "udrt"), 1_000_000);
        assert_eq!(state.nonce_of("owner"), 0);
    }

    #[test]
    fn test_upfront_fee_calculation() {
        let ctx = ExecutionContext::new(25000, 1500);
        let fee = ctx.calculate_upfront_fee().unwrap();
        assert_eq!(fee, 37_500_000); // 25000 * 1500
    }

    #[test]
    fn test_upfront_fee_overflow() {
        let ctx = ExecutionContext::new(u64::MAX, u64::MAX);
        // Multiplying two u64 values fits into u128 without overflow
        let fee = ctx
            .calculate_upfront_fee()
            .expect("no overflow for u64::MAX * u64::MAX");
        let expected = (u128::from(u64::MAX)) * (u128::from(u64::MAX));
        assert_eq!(fee, expected);
    }

    #[test]
    fn test_successful_execution() {
        let (mut state, _directory) = create_test_state();
        let gas_schedule = GasSchedule::default();

        // Setup initial state
        state.set_balance("alice", "udgt", 100_000);
        state.set_balance("alice", "udrt", 100_000); // Current runtime charges gas in udrt.
        state.set_balance("bob", "udgt", 50_000);

        let tx = Transaction::new(
            "test_hash".to_string(),
            "alice".to_string(),
            "bob".to_string(),
            1_000,
            10_000,
            0,
            Some("sig".to_string()),
        )
        .with_gas(25_000, 1);

        let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

        assert!(result.success);
        assert_eq!(result.receipt.status, TxStatus::Success);
        assert!(result.gas_used > 0);
        assert_eq!(result.receipt.gas_limit, 25_000);
        assert_eq!(result.receipt.gas_price, 1);
        assert_eq!(state.balance_of("alice", "udgt"), 99_000);
        assert_eq!(state.balance_of("bob", "udgt"), 51_000);
        assert_eq!(state.balance_of("alice", "udrt"), 75_000);
    }

    #[test]
    fn test_insufficient_funds() {
        let (mut state, _directory) = create_test_state();
        let gas_schedule = GasSchedule::default();

        // Setup insufficient balance
        state.set_balance("alice", "udgt", 1_000); // Not enough for amount + gas

        let tx = Transaction::new(
            "test_hash".to_string(),
            "alice".to_string(),
            "bob".to_string(),
            1_000,
            10_000,
            0,
            Some("sig".to_string()),
        )
        .with_gas(25_000, 1_000); // High gas price

        let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

        assert!(!result.success);
        assert_eq!(result.receipt.status, TxStatus::Failed);
        assert!(result
            .receipt
            .error
            .as_ref()
            .unwrap()
            .contains("InsufficientFunds"));
    }
}
