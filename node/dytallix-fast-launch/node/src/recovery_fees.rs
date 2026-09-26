//! Explicit local sponsored-recovery overlay. No migration or ordinary envelope fallback.
//! Native Settlement balances remain authoritative. The caller commits the book,
//! native fee writes, receipts and block head in the same synchronous database batch.
use crate::settlement::Settlement;
use anyhow::{bail, ensure, Context, Result};
use dytallix_protocol_types::{
    recovery::{ActionKind, KeyIdentity, RecoveryPolicy, RecoveryState},
    recovery_sponsor::{self as wire, FeeProfile, SponsoredRecovery},
    sha3_256,
};
use dytallix_runtime_crypto::recovery_sponsor::{verify_signed, SponsorVerificationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const STATE_KEY: &str = "recovery:v1:book";
// Implementation bounds for the isolated local profile. No pruning is authorized.
pub(crate) const MAX_ACCOUNTS: usize = 4096;
const MAX_RECEIPTS: usize = 65536;
const MAX_BOOK_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAccount {
    pub address: String,
    pub recovery: RecoveryState,
    pub sponsor_nonce: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SponsorReceipt {
    pub operation_id: [u8; 32],
    pub sponsor_authorization_id: [u8; 32],
    pub envelope_hash: [u8; 32],
    pub sponsor_account_id: [u8; 32],
    pub target_account_id: [u8; 32],
    pub block_height: u64,
    pub block_index: u32,
    pub success: bool,
    pub profile_version: u64,
    pub profile_digest: [u8; 32],
    pub gas_limit: u64,
    pub gas_used: u64,
    pub reserved_cap: u128,
    pub settled_fee: u128,
    pub released_reserve: u128,
    pub sponsor_counter_before: u64,
    pub sponsor_counter_after: u64,
    pub target_state_digest: [u8; 32],
    pub record_hash: [u8; 32],
}
impl SponsorReceipt {
    fn hash(&self) -> Result<[u8; 32]> {
        let mut copy = self.clone();
        copy.record_hash = [0; 32];
        let mut bytes = b"DYTALLIX/RECOVERY-RECEIPT\0".to_vec();
        bytes.extend(serde_json::to_vec(&copy)?);
        Ok(sha3_256(&bytes))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBook {
    pub version: u16,
    pub last_height: u64,
    pub profile: FeeProfile,
    pub accounts: BTreeMap<String, RecoveryAccount>,
    pub sponsor_receipts: BTreeMap<String, SponsorReceipt>,
    pub operation_success: BTreeMap<String, String>,
    pub expiry_index: BTreeMap<u64, BTreeSet<String>>,
}
impl RecoveryBook {
    pub(crate) fn new(profile: FeeProfile, accounts: Vec<RecoveryAccount>) -> Result<Self> {
        let mut book = Self {
            version: 1,
            last_height: 0,
            profile,
            accounts: BTreeMap::new(),
            sponsor_receipts: BTreeMap::new(),
            operation_success: BTreeMap::new(),
            expiry_index: BTreeMap::new(),
        };
        for account in accounts {
            ensure!(
                account.sponsor_nonce == 0 && account.recovery.last_height == 0,
                "Fresh recovery genesis requires zero height and sponsor counters"
            );
            ensure!(
                account.recovery.pending_recovery.is_none()
                    && account.recovery.pending_policy.is_none(),
                "Fresh recovery genesis cannot contain pending work"
            );
            let id = hex::encode(account.recovery.domain.account_id);
            ensure!(
                book.accounts.insert(id, account).is_none(),
                "Duplicate recovery account ID"
            );
        }
        book.validate()?;
        Ok(book)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported recovery book version");
        self.profile.validate()?;
        ensure!(
            !self.accounts.is_empty() && self.accounts.len() <= MAX_ACCOUNTS,
            "Recovery account count outside bounds"
        );
        ensure!(
            self.sponsor_receipts.len() <= MAX_RECEIPTS,
            "Recovery receipt capacity exceeded"
        );
        let profile_digest = wire::profile_digest(&self.profile)?;
        let mut addresses = BTreeSet::new();
        let mut domain = None;
        for (id, account) in &self.accounts {
            account.recovery.validate()?;
            ensure!(
                *id == hex::encode(account.recovery.domain.account_id),
                "Recovery account key mismatch"
            );
            ensure!(
                !account.address.is_empty()
                    && account.address.len() <= 256
                    && addresses.insert(&account.address),
                "Invalid or duplicate native account mapping"
            );
            ensure!(
                account.recovery.last_height == self.last_height,
                "Recovery account height differs from book"
            );
            let d = &account.recovery.domain;
            let shared = (d.network, d.chain_id.clone(), d.genesis_digest);
            if let Some(expected) = &domain {
                ensure!(*expected == shared, "Mixed recovery chain domains");
            }
            domain = Some(shared);
            for (algorithm, length) in &account.recovery.config.algorithms {
                let expected = match algorithm.as_str() {
                    "mldsa65" => 1952,
                    "mldsa87" => 2592,
                    _ => bail!("Unsupported configured recovery algorithm"),
                };
                ensure!(
                    *length == expected && self.profile.signature_costs.contains_key(algorithm),
                    "Recovery key profile differs from fee profile"
                );
            }
        }
        let expected = self.expected_expiries()?;
        ensure!(
            expected == self.expiry_index,
            "Recovery due-expiry index differs from obligations"
        );
        self.check_capacity(&expected)?;
        let mut positions = BTreeSet::new();
        let mut counters = BTreeSet::new();
        for (id, receipt) in &self.sponsor_receipts {
            ensure!(
                *id == hex::encode(receipt.sponsor_authorization_id)
                    && receipt.record_hash == receipt.hash()?,
                "Recovery receipt digest mismatch"
            );
            ensure!(
                receipt.block_height > 0
                    && receipt.block_height <= self.last_height
                    && positions.insert((receipt.block_height, receipt.block_index)),
                "Invalid receipt block position"
            );
            ensure!(
                receipt.profile_version == self.profile.version
                    && receipt.profile_digest == profile_digest,
                "Receipt requires an unavailable fee profile"
            );
            ensure!(
                receipt.gas_limit > 0
                    && receipt.gas_limit <= self.profile.max_transaction_gas
                    && receipt.gas_used <= receipt.gas_limit
                    && receipt.gas_used >= self.profile.minimum_gas,
                "Invalid receipt gas counters"
            );
            ensure!(
                receipt.success || receipt.gas_used == receipt.gas_limit,
                "Invalid charged failure"
            );
            ensure!(
                receipt.settled_fee
                    == u128::from(receipt.gas_used) * u128::from(self.profile.gas_price)
                    && receipt.reserved_cap.checked_sub(receipt.settled_fee)
                        == Some(receipt.released_reserve)
                    && receipt.reserved_cap <= self.profile.max_fee_cap
                    && receipt.reserved_cap
                        >= u128::from(receipt.gas_limit) * u128::from(self.profile.gas_price),
                "Invalid receipt fee conservation"
            );
            ensure!(
                receipt.sponsor_counter_before.checked_add(1)
                    == Some(receipt.sponsor_counter_after),
                "Invalid receipt sponsor counter"
            );
            let sponsor_id = hex::encode(receipt.sponsor_account_id);
            let sponsor = self
                .accounts
                .get(&sponsor_id)
                .context("Missing receipt sponsor")?;
            ensure!(
                receipt.sponsor_counter_after <= sponsor.sponsor_nonce
                    && counters.insert((sponsor_id, receipt.sponsor_counter_before)),
                "Repeated or future sponsor counter"
            );
            ensure!(
                self.accounts
                    .contains_key(&hex::encode(receipt.target_account_id))
                    && receipt.target_account_id != receipt.sponsor_account_id,
                "Invalid receipt target"
            );
            let success = self
                .operation_success
                .get(&hex::encode(receipt.operation_id));
            ensure!(
                !receipt.success || success == Some(id),
                "Successful receipt absent from operation index"
            );
            ensure!(
                receipt.success || success != Some(id),
                "Failure entered successful operation index"
            );
        }
        for (operation, id) in &self.operation_success {
            let receipt = self
                .sponsor_receipts
                .get(id)
                .context("Success index references absent receipt")?;
            ensure!(
                receipt.success && *operation == hex::encode(receipt.operation_id),
                "Success index mismatch"
            );
        }
        // All sponsorship history is retained. Gaps cannot be silently accepted.
        for (id, account) in &self.accounts {
            let count = counters.iter().filter(|(sponsor, _)| sponsor == id).count() as u64;
            ensure!(
                count == account.sponsor_nonce,
                "Sponsor history does not cover its counter"
            );
        }
        Ok(())
    }
    fn expected_expiries(&self) -> Result<BTreeMap<u64, BTreeSet<String>>> {
        let mut index: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
        for (id, account) in &self.accounts {
            for height in [
                account
                    .recovery
                    .pending_recovery
                    .as_ref()
                    .map(|p| p.expiry_height),
                account
                    .recovery
                    .pending_policy
                    .as_ref()
                    .map(|p| p.expiry_height),
            ]
            .into_iter()
            .flatten()
            {
                ensure!(
                    height > self.last_height,
                    "Unprocessed mandatory recovery expiry"
                );
                ensure!(
                    index.entry(height).or_default().insert(id.clone()),
                    "Duplicate account expiry obligation"
                );
            }
        }
        Ok(index)
    }
    fn capacity_available(&self, index: &BTreeMap<u64, BTreeSet<String>>) -> Result<bool> {
        let pending: BTreeSet<_> = index.values().flat_map(|ids| ids.iter()).collect();
        if pending.len() as u64 > self.profile.max_pending_accounts {
            return Ok(false);
        }
        for ids in index.values() {
            if ids.len() as u64 > self.profile.max_due_expiry_events_per_height {
                return Ok(false);
            }
            let gas = (ids.len() as u64)
                .checked_mul(self.profile.expiry_event_gas_cost)
                .context("Expiry cost overflow")?;
            if gas > self.profile.mandatory_expiry_gas_budget {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn check_capacity(&self, index: &BTreeMap<u64, BTreeSet<String>>) -> Result<()> {
        ensure!(
            self.capacity_available(index)?,
            "Recovery expiry capacity exhausted"
        );
        Ok(())
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_BOOK_BYTES,
            "Recovery book size exceeds bound"
        );
        let book: Self = serde_json::from_slice(bytes).context("Invalid recovery book")?;
        book.validate()?;
        ensure!(
            serde_json::to_vec(&book)? == bytes,
            "Recovery book encoding is not canonical"
        );
        Ok(book)
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() <= MAX_BOOK_BYTES,
            "Recovery book size exceeds bound"
        );
        Ok(bytes)
    }
    pub(crate) fn begin_block(&self, height: u64) -> Result<RecoveryBlock> {
        self.validate()?;
        ensure!(
            height
                == self
                    .last_height
                    .checked_add(1)
                    .context("Recovery height overflow")?,
            "Recovery blocks must be consecutive"
        );
        #[cfg(not(feature = "pqc-fips204"))]
        bail!("Recovery profile requires the FIPS 204 backend");
        #[cfg(feature = "pqc-fips204")]
        {
            let mut book = self.clone();
            let due = self.expiry_index.get(&height).map_or(0, BTreeSet::len) as u64;
            let expiry_gas_used = due
                .checked_mul(self.profile.expiry_event_gas_cost)
                .context("Expiry budget overflow")?;
            ensure!(
                expiry_gas_used <= self.profile.mandatory_expiry_gas_budget,
                "Mandatory expiry exceeds reserved budget"
            );
            for account in book.accounts.values_mut() {
                account.recovery = account.recovery.advance_height(height)?;
            }
            book.last_height = height;
            book.expiry_index = book.expected_expiries()?;
            book.validate()?;
            Ok(RecoveryBlock {
                book,
                gas_used: 0,
                bytes_used: 0,
                signatures_used: 0,
                expiry_gas_used,
            })
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct RecoveryBlock {
    pub book: RecoveryBook,
    pub gas_used: u64,
    pub bytes_used: u64,
    pub signatures_used: u64,
    pub expiry_gas_used: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecoveryResult {
    pub success: bool,
    pub charged: bool,
    pub gas_used: u64,
    pub fee: u128,
    pub error: Option<String>,
    pub receipt: Option<SponsorReceipt>,
}
impl RecoveryResult {
    fn rejected(gas: u64, reason: impl Into<String>) -> Self {
        Self {
            success: false,
            charged: false,
            gas_used: gas,
            fee: 0,
            error: Some(reason.into()),
            receipt: None,
        }
    }
}
/// Reconcile one charged recovery against its staged native custody delta.
/// Recovery may charge the sponsor, but it may not change a native nonce or
/// any other native account balance at this execution boundary.
fn reconcile_sponsor_charge(
    before: &mut Settlement,
    after: &mut Settlement,
    sponsor: &RecoveryAccount,
    receipt: &SponsorReceipt,
    gas_price: u64,
) -> Result<()> {
    ensure!(
        receipt.sponsor_account_id == sponsor.recovery.domain.account_id
            && receipt.sponsor_counter_before == sponsor.sponsor_nonce
            && Some(receipt.sponsor_counter_after) == sponsor.sponsor_nonce.checked_add(1)
            && receipt.settled_fee
                == u128::from(receipt.gas_used)
                    .checked_mul(u128::from(gas_price))
                    .context("Recovery receipt fee overflow")?
            && receipt.reserved_cap.checked_sub(receipt.settled_fee)
                == Some(receipt.released_reserve),
        "Recovery receipt counter or fee differs from charge"
    );
    let prior_fee = before.ordinary_fee_total()?;
    ensure!(
        prior_fee.checked_add(receipt.settled_fee) == Some(after.ordinary_fee_total()?),
        "Recovery receipt differs from withheld fee delta"
    );
    let initial = before.account(&sponsor.address)?.clone();
    let final_account = after.account(&sponsor.address)?.clone();
    let mut expected = initial;
    expected.set_balance(
        "udrt",
        expected
            .balance_of("udrt")
            .checked_sub(receipt.settled_fee)
            .context("Recovery charge exceeds sponsor balance")?,
    );
    ensure!(
        final_account.nonce == expected.nonce && final_account.balances == expected.balances,
        "Recovery receipt differs from sponsor account delta"
    );
    ensure!(
        before.accounts.keys().eq(after.accounts.keys()),
        "Recovery charge changed native account set"
    );
    for (address, initial) in &before.accounts {
        if address == &sponsor.address {
            continue;
        }
        let final_account = &after.accounts[address];
        ensure!(
            final_account.nonce == initial.nonce && final_account.balances == initial.balances,
            "Recovery charge changed unrelated native account"
        );
    }
    Ok(())
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("Recovery meter overflow")
}
fn cost(bytes: u64, price: u64) -> Result<u64> {
    bytes
        .checked_mul(price)
        .context("Recovery meter multiplication overflow")
}
fn action_index(action: &ActionKind) -> Result<usize> {
    Ok(match action {
        ActionKind::Enroll { .. } => 0,
        ActionKind::Rotate { .. } => 1,
        ActionKind::Start { .. } => 2,
        ActionKind::Finalize { .. } => 3,
        ActionKind::Cancel { .. } => 4,
        ActionKind::Resume { .. } => 5,
        ActionKind::StagePolicy { .. } => 6,
        ActionKind::ActivatePolicy { .. } => 7,
        ActionKind::CancelPolicy { .. } => 8,
        ActionKind::Spend { .. } => bail!("Internal Spend is not a complete external operation"),
    })
}
fn key_size(key: &KeyIdentity) -> u64 {
    3 + key.public_key.len() as u64
}
fn policy_size(policy: &RecoveryPolicy) -> Result<u64> {
    policy.guardians.iter().try_fold(3, |size, g| {
        add(
            size,
            add(key_size(&g.key), 2 + g.control_group.len() as u64)?,
        )
    })
}
/// Logical bytes, independent of JSON integer width, database layout and compression.
/// Domain, config, current key/counters/status, policies and pending terminal records.
pub(crate) fn logical_account_bytes(account: &RecoveryAccount) -> Result<u64> {
    let state = &account.recovery;
    let mut size =
        2 + account.address.len() as u64 + 8 + 1 + 2 + state.domain.chain_id.len() as u64 + 64;
    size = add(size, 6 * 8 + 1 + key_size(&state.active_key) + 6 * 8 + 1)?;
    for (name, _) in &state.config.algorithms {
        size = add(size, 2 + name.len() as u64 + 8)?;
    }
    size = add(size, 1)?;
    if let Some(policy) = &state.policy {
        size = add(size, policy_size(policy)?)?;
    }
    size = add(size, 2)?;
    if let Some(p) = &state.pending_recovery {
        size = add(size, 32 + key_size(&p.replacement) + 5 * 8)?;
    }
    if let Some(p) = &state.pending_policy {
        size = add(size, 32 + policy_size(&p.policy)? + 5 * 8)?;
    }
    Ok(size)
}
impl RecoveryBlock {
    fn consume(
        &mut self,
        gas: u64,
        bytes: u64,
        signatures: u64,
        shared: &mut Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<()> {
        let g = add(self.gas_used, gas)?;
        let b = add(self.bytes_used, bytes)?;
        let s = add(self.signatures_used, signatures)?;
        ensure!(
            g <= self.book.profile.max_block_gas
                && b <= self.book.profile.max_block_recovery_bytes
                && s <= self.book.profile.max_block_recovery_signatures,
            "Recovery block capacity exceeded"
        );
        // Check the combined ceiling before permitting the corresponding work.
        // Failure leaves this subtotal unchanged and requires block rejection.
        if let Some(meter) = shared.as_deref_mut() {
            meter.charge_recovery(gas, bytes, signatures)?;
        }
        self.gas_used = g;
        self.bytes_used = b;
        self.signatures_used = s;
        Ok(())
    }
    pub(crate) fn execute(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
    ) -> Result<RecoveryResult> {
        self.execute_inner(height, index, raw, settlement, None)
    }
    /// Combined ordinary/recovery profile. Each increment checks both the
    /// recovery subtotal and shared ceiling before signature or transition work.
    /// Mandatory expiry is charged separately once when the block starts.
    pub(crate) fn execute_with_shared(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
        shared: &mut crate::ordinary_meter::SharedBlockMeter,
    ) -> Result<RecoveryResult> {
        self.execute_inner(height, index, raw, settlement, Some(shared))
    }
    fn execute_inner(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
        mut shared: Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<RecoveryResult> {
        ensure!(
            height == self.book.last_height,
            "Recovery execution height mismatch"
        );
        let p = self.book.profile.clone();
        let byte_gas = cost(raw.len() as u64, p.wire_byte_cost)?;
        self.consume(byte_gas, raw.len() as u64, 0, &mut shared)?;
        let signed = match wire::decode(raw) {
            Ok(v) => v,
            Err(e) => return Ok(RecoveryResult::rejected(byte_gas, e.to_string())),
        };
        self.execute_decoded(
            height,
            index,
            raw,
            signed,
            byte_gas,
            settlement,
            &mut shared,
        )
    }
    fn execute_decoded(
        &mut self,
        height: u64,
        index: u32,
        _raw: &[u8],
        signed: SponsoredRecovery,
        byte_gas: u64,
        settlement: &mut Settlement,
        shared: &mut Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<RecoveryResult> {
        let p = self.book.profile.clone();
        if height < p.activation_height {
            return Ok(RecoveryResult::rejected(
                byte_gas,
                "Recovery fee profile not active",
            ));
        }
        let a = &signed.sponsor;
        let mut gas = byte_gas;
        let action_cost = p.action_costs[action_index(&signed.recovery.operation.action.kind)?];
        self.consume(action_cost, 0, 0, shared)?;
        gas = add(gas, action_cost)?;
        // Reserve all declared signature work before executing the verifier. This is
        // conservative when an early signature fails, and independent of CPU timing.
        let keys = signed
            .recovery
            .signatures
            .iter()
            .map(|s| &s.key)
            .chain(std::iter::once(&a.sponsor_key));
        let mut signature_gas = 0;
        for key in keys {
            let Some(price) = p.signature_costs.get(&key.algorithm) else {
                return Ok(RecoveryResult::rejected(
                    gas,
                    "Requested algorithm absent from fee profile",
                ));
            };
            signature_gas = add(signature_gas, *price)?;
        }
        self.consume(
            signature_gas,
            0,
            signed.recovery.signatures.len() as u64 + 1,
            shared,
        )?;
        gas = add(gas, signature_gas)?;
        macro_rules! reject {
            ($condition:expr,$reason:expr) => {
                if !$condition {
                    return Ok(RecoveryResult::rejected(gas, $reason));
                }
            };
        }
        reject!(
            a.fee_profile_version == p.version
                && a.fee_profile_digest == wire::profile_digest(&p)?
                && a.denomination == p.denomination,
            "Fee profile binding mismatch"
        );
        reject!(
            a.gas_limit > 0
                && a.gas_limit >= p.minimum_gas
                && a.gas_limit <= p.max_transaction_gas
                && gas <= a.gas_limit,
            "Validation exceeds gas limit"
        );
        let maximum = u128::from(a.gas_limit) * u128::from(p.gas_price);
        reject!(
            a.maximum_charge >= maximum && a.maximum_charge <= p.max_fee_cap,
            "Fee cap outside profile bounds"
        );
        reject!(
            height < a.expiry_height && height < signed.recovery.operation.action.submission_expiry,
            "Expired sponsorship or operation"
        );
        let facts = match verify_signed(&signed) {
            Ok(facts) => facts,
            Err(SponsorVerificationError::BackendUnavailable) => {
                bail!("Required recovery verifier backend unavailable")
            }
            Err(e) => return Ok(RecoveryResult::rejected(gas, e.to_string())),
        };
        let target_id = hex::encode(a.domain.account_id);
        let sponsor_id = hex::encode(a.sponsor_account_id);
        reject!(
            target_id != sponsor_id,
            "Recovery sponsor must be independent"
        );
        let Some(target) = self.book.accounts.get(&target_id).cloned() else {
            return Ok(RecoveryResult::rejected(gas, "Unknown recovery target"));
        };
        let Some(sponsor) = self.book.accounts.get(&sponsor_id).cloned() else {
            return Ok(RecoveryResult::rejected(gas, "Unknown recovery sponsor"));
        };
        let auth_id = wire::authorization_id(a)?;
        let auth_key = hex::encode(auth_id);
        let op_key = hex::encode(a.operation_id);
        reject!(
            !self.book.sponsor_receipts.contains_key(&auth_key)
                && !self.book.operation_success.contains_key(&op_key),
            "Consumed sponsorship or successful operation"
        );
        reject!(
            self.book.sponsor_receipts.len() < MAX_RECEIPTS,
            "Receipt retention capacity exhausted"
        );
        reject!(target.recovery.domain == a.domain, "Target domain mismatch");
        reject!(
            sponsor.recovery.outgoing_allowed()
                && sponsor.recovery.active_key == a.sponsor_key
                && sponsor.recovery.active_generation == a.sponsor_generation
                && sponsor.sponsor_nonce == a.sponsor_nonce
                && sponsor.sponsor_nonce < u64::MAX,
            "Sponsor current authority mismatch or protection active"
        );
        let read_bytes = add(
            add(
                logical_account_bytes(&target)?,
                logical_account_bytes(&sponsor)?,
            )?,
            32,
        )?; // native liquid and custody counters
        let read_gas = cost(read_bytes, p.read_byte_cost)?;
        self.consume(read_gas, 0, 0, shared)?;
        gas = add(gas, read_gas)?;
        reject!(
            gas <= a.gas_limit,
            "Validation state reads exceed gas limit"
        );
        let next = match target.recovery.transition(height, &facts.action, &facts) {
            Ok(v) => v,
            Err(e) => return Ok(RecoveryResult::rejected(gas, e.to_string())),
        };
        let mut proposed = self.book.clone();
        proposed
            .accounts
            .get_mut(&target_id)
            .expect("validated target")
            .recovery = next.clone();
        proposed.expiry_index = proposed.expected_expiries()?;
        reject!(
            proposed.capacity_available(&proposed.expiry_index)?,
            "Future expiry capacity unavailable"
        );
        let mut native = settlement.clone();
        reject!(
            native.account(&sponsor.address)?.balance_of("udrt") >= a.maximum_charge,
            "Insufficient eligible sponsor liquidity"
        );
        // Fee acceptance boundary. All authority, timing and capacity checks passed.
        // Only deterministic final write metering can produce the charged failure.
        let mut next_sponsor = sponsor.clone();
        next_sponsor.sponsor_nonce += 1;
        let write_bytes = add(
            add(
                logical_account_bytes(proposed.accounts.get(&target_id).expect("target"))?,
                logical_account_bytes(&next_sponsor)?,
            )?,
            32,
        )?;
        let write_gas = cost(write_bytes, p.write_byte_cost)?;
        let requested = add(gas, write_gas)?;
        let success = requested <= a.gas_limit;
        let final_gas = if success {
            requested.max(p.minimum_gas)
        } else {
            a.gas_limit
        };
        self.consume(
            final_gas
                .checked_sub(gas)
                .context("Gas acceptance invariant")?,
            0,
            0,
            shared,
        )?;
        let fee = u128::from(final_gas) * u128::from(p.gas_price);
        let mut receipt = SponsorReceipt {
            operation_id: a.operation_id,
            sponsor_authorization_id: auth_id,
            envelope_hash: wire::envelope_hash(&signed)?,
            sponsor_account_id: a.sponsor_account_id,
            target_account_id: a.domain.account_id,
            block_height: height,
            block_index: index,
            success,
            profile_version: p.version,
            profile_digest: wire::profile_digest(&p)?,
            gas_limit: a.gas_limit,
            gas_used: final_gas,
            reserved_cap: a.maximum_charge,
            settled_fee: fee,
            released_reserve: a
                .maximum_charge
                .checked_sub(fee)
                .context("Fee exceeds reservation")?,
            sponsor_counter_before: sponsor.sponsor_nonce,
            sponsor_counter_after: next_sponsor.sponsor_nonce,
            target_state_digest: sha3_256(&serde_json::to_vec(if success {
                &next
            } else {
                &target.recovery
            })?),
            record_hash: [0; 32],
        };
        receipt.record_hash = receipt.hash()?;
        let mut committed = if success { proposed } else { self.book.clone() };
        committed.accounts.insert(sponsor_id, next_sponsor);
        committed
            .sponsor_receipts
            .insert(auth_key.clone(), receipt.clone());
        if success {
            committed.operation_success.insert(op_key, auth_key);
        }
        // Validate before mutating native custody. Encoding bounds are also checked.
        committed.encode()?;
        let mut baseline = settlement.clone();
        native.charge_sponsored(&sponsor.address, fee)?;
        reconcile_sponsor_charge(&mut baseline, &mut native, &sponsor, &receipt, p.gas_price)?;
        *settlement = native;
        self.book = committed;
        Ok(RecoveryResult {
            success,
            charged: true,
            gas_used: final_gas,
            fee,
            error: if success {
                None
            } else {
                Some("Accepted recovery out of gas".into())
            },
            receipt: Some(receipt),
        })
    }
}
/// Queue-only reservations. No chain state changes or anticipated credits.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecoveryReservations {
    ordinary: BTreeMap<String, u128>,
    sponsors: BTreeMap<String, (String, u64, u128)>,
    nonces: BTreeMap<(String, u64), String>,
}
impl RecoveryReservations {
    fn reserved(&self, address: &str) -> Result<u128> {
        self.sponsors
            .values()
            .filter(|(a, _, _)| a == address)
            .try_fold(
                self.ordinary.get(address).copied().unwrap_or(0),
                |total, (_, _, cap)| {
                    total
                        .checked_add(*cap)
                        .context("Queue reservation overflow")
                },
            )
    }
    pub(crate) fn reserve_ordinary(
        &mut self,
        address: &str,
        amount: u128,
        liquid: u128,
    ) -> Result<()> {
        let total = self
            .reserved(address)?
            .checked_add(amount)
            .context("Queue reservation overflow")?;
        ensure!(total <= liquid, "Queue exceeds current liquid balance");
        let old = self.ordinary.get(address).copied().unwrap_or(0);
        self.ordinary.insert(
            address.into(),
            old.checked_add(amount)
                .context("Ordinary reservation overflow")?,
        );
        Ok(())
    }
    pub(crate) fn reserve_sponsor(
        &mut self,
        id: [u8; 32],
        address: &str,
        nonce: u64,
        cap: u128,
        liquid: u128,
    ) -> Result<bool> {
        let id = hex::encode(id);
        let value = (address.to_owned(), nonce, cap);
        if let Some(old) = self.sponsors.get(&id) {
            ensure!(*old == value, "Queue authorization identity mismatch");
            return Ok(false);
        }
        ensure!(
            !self.nonces.contains_key(&(address.into(), nonce)),
            "Sponsor nonce already reserved"
        );
        ensure!(
            self.reserved(address)?
                .checked_add(cap)
                .context("Queue reservation overflow")?
                <= liquid,
            "Queue exceeds current liquid balance"
        );
        self.nonces.insert((address.into(), nonce), id.clone());
        self.sponsors.insert(id, value);
        Ok(true)
    }
}
#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "recovery_fees_tests.rs"]
mod tests;

#[cfg(all(test, not(feature = "pqc-fips204")))]
#[path = "recovery_backend_tests.rs"]
mod backend_tests;
