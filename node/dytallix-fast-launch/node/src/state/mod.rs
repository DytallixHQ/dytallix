use crate::storage::state::Storage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Fixed maximum supply of DGT, expressed in micro-units (udgt).
/// 1,000,000,000 DGT * 1_000_000 udgt/DGT = 1e15 udgt. DGT is a fixed-supply
/// governance token: it is fully allocated at genesis and never minted beyond
/// this ceiling. (DRT, the reward/fee token, is uncapped and not affected.)
pub const DGT_MAX_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;

/// Storage key for the cumulative amount of DGT minted (genesis + any later mint).
pub(crate) const DGT_MINTED_KEY: &str = "supply:dgt_minted";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub balances: BTreeMap<String, u128>, // Multi-denomination balances
    pub nonce: u64,
}

impl AccountState {
    /// Get balance for a specific denomination
    pub fn balance_of(&self, denom: &str) -> u128 {
        self.balances.get(denom).copied().unwrap_or(0)
    }

    /// Get legacy single balance (defaults to udgt for backward compatibility)
    pub fn legacy_balance(&self) -> u128 {
        self.balance_of("udgt")
    }

    /// Set balance for a specific denomination
    pub fn set_balance(&mut self, denom: &str, amount: u128) {
        if amount == 0 {
            self.balances.remove(denom);
        } else {
            self.balances.insert(denom.to_string(), amount);
        }
    }

    /// Add to balance for a specific denomination
    pub fn add_balance(&mut self, denom: &str, amount: u128) {
        let current = self.balance_of(denom);
        self.set_balance(denom, current.saturating_add(amount));
    }

    /// Subtract from balance for a specific denomination
    pub fn sub_balance(&mut self, denom: &str, amount: u128) -> Result<(), String> {
        let current = self.balance_of(denom);
        if current < amount {
            return Err(format!(
                "Insufficient balance in {denom}: {current} < {amount}"
            ));
        }
        self.set_balance(denom, current - amount);
        Ok(())
    }
}

#[derive(Clone)]
pub struct State {
    pub accounts: HashMap<String, AccountState>,
    pub storage: Arc<Storage>,
}

impl Default for State {
    fn default() -> Self {
        // Fallback default uses an in-memory temporary directory only when tests build with dev-dependency tempfile.
        // To avoid compile error when tempfile crate not present, gate this with cfg(test).
        #[cfg(test)]
        {
            let tmp = tempfile::tempdir().expect("failed to create temp dir for default state");
            let storage = Storage::open(tmp.path().to_path_buf())
                .expect("failed to open storage for default state");
            Self {
                accounts: HashMap::new(),
                storage: Arc::new(storage),
            }
        }
        #[cfg(not(test))]
        {
            panic!(
                "State::default is only available in tests; use State::new with a Storage instance"
            );
        }
    }
}

impl State {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            accounts: HashMap::new(),
            storage,
        }
    }

    /// Test helper: create state without storage for testing
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        use std::sync::Arc;
        use tempfile::TempDir;

        // Create a temporary storage for testing
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let storage = Arc::new(
            Storage::open(temp_dir.path().to_path_buf()).expect("Failed to create storage"),
        );

        Self {
            accounts: HashMap::new(),
            storage,
        }
    }

    pub fn get_account(&mut self, addr: &str) -> AccountState {
        // lazy load DB
        if let Some(a) = self.accounts.get(addr) {
            return a.clone();
        }
        let balances = self.storage.get_balances_db(addr);
        let nonce = self.storage.get_nonce_db(addr);
        let a = AccountState { balances, nonce };
        self.accounts.insert(addr.to_string(), a.clone());
        a
    }

    /// Lightweight read-only snapshot of an account state without mutating caches.
    /// Uses in-memory cache if present, otherwise reads directly from storage.
    pub fn snapshot_account(&self, addr: &str) -> AccountState {
        if let Some(a) = self.accounts.get(addr) {
            a.clone()
        } else {
            let balances = self.storage.get_balances_db(addr);
            let nonce = self.storage.get_nonce_db(addr);
            AccountState { balances, nonce }
        }
    }

    /// Get balance for specific denomination
    pub fn balance_of(&mut self, addr: &str, denom: &str) -> u128 {
        self.get_account(addr).balance_of(denom)
    }

    /// Get legacy single balance (backward compatibility)
    pub fn legacy_balance_of(&mut self, addr: &str) -> u128 {
        self.get_account(addr).legacy_balance()
    }

    /// Get all balances for an address
    pub fn balances_of(&mut self, addr: &str) -> BTreeMap<String, u128> {
        self.get_account(addr).balances.clone()
    }

    pub fn nonce_of(&mut self, addr: &str) -> u64 {
        self.get_account(addr).nonce
    }

    /// Read-only helpers that avoid &mut and cloning the whole State repeatedly
    pub fn snapshot_legacy_balance(&self, addr: &str) -> u128 {
        self.snapshot_account(addr).legacy_balance()
    }

    pub fn snapshot_nonce(&self, addr: &str) -> u64 {
        self.accounts
            .get(addr)
            .map(|account| account.nonce)
            .unwrap_or_else(|| self.storage.get_nonce_db(addr))
    }

    /// Apply transfer with specific denomination
    pub fn apply_transfer(
        &mut self,
        from: &str,
        to: &str,
        denom: &str,
        amount: u128,
        fee_denom: &str,
        fee: u128,
    ) -> Result<(), String> {
        let mut sender = self.get_account(from);

        // Subtract amount from sender
        sender.sub_balance(denom, amount)?;
        // Subtract fee from sender (fees are always in fee_denom, typically "udgt")
        sender.sub_balance(fee_denom, fee)?;
        sender.nonce += 1;

        let mut recv = self.get_account(to);
        recv.add_balance(denom, amount);

        // Update in-memory cache
        self.accounts.insert(from.to_string(), sender.clone());
        self.accounts.insert(to.to_string(), recv.clone());

        // Persist to storage
        let _ = self.storage.set_balances_db(from, &sender.balances);
        let _ = self.storage.set_nonce_db(from, sender.nonce);
        let _ = self.storage.set_balances_db(to, &recv.balances);

        Ok(())
    }

    /// Legacy apply_transfer for backward compatibility. Transfers the value in
    /// udgt, but charges the fee in udrt (DRT is the reward/fee token).
    pub fn apply_transfer_legacy(&mut self, from: &str, to: &str, amount: u128, fee: u128) {
        // Convert to new multi-denom format
        let _ = self.apply_transfer(from, to, "udgt", amount, "udrt", fee);
    }

    /// Credit specific denomination to an address
    pub fn credit(&mut self, addr: &str, denom: &str, amount: u128) {
        let mut a = self.get_account(addr);
        a.add_balance(denom, amount);
        self.accounts.insert(addr.to_string(), a.clone());
        let _ = self.storage.set_balances_db(addr, &a.balances);
    }

    /// Legacy credit for backward compatibility (uses udgt as default)
    pub fn credit_legacy(&mut self, addr: &str, amount: u128) {
        self.credit(addr, "udgt", amount);
    }

    /// Cumulative DGT minted so far (in udgt), persisted across restarts.
    pub fn dgt_total_minted(&self) -> u128 {
        self.storage
            .db
            .get(DGT_MINTED_KEY)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize::<u128>(&v).ok())
            .unwrap_or(0)
    }

    fn set_dgt_total_minted(&self, total: u128) {
        if let Ok(bytes) = bincode::serialize(&total) {
            let _ = self.storage.db.put(DGT_MINTED_KEY, bytes);
        }
    }

    /// Mint DGT (udgt) to an address, enforcing the fixed `DGT_MAX_SUPPLY` cap.
    /// Runtime mint path. Atomic genesis import separately enforces the same cap.
    /// Returns an error (minting nothing) if
    /// the mint would push cumulative DGT past the cap.
    pub fn mint_dgt(&mut self, addr: &str, amount: u128) -> Result<u128, String> {
        let minted = self.dgt_total_minted();
        let new_total = minted
            .checked_add(amount)
            .ok_or_else(|| "DGT mint overflow".to_string())?;
        if new_total > DGT_MAX_SUPPLY {
            return Err(format!(
                "DGT supply cap exceeded: {minted} + {amount} > {DGT_MAX_SUPPLY}"
            ));
        }
        self.credit(addr, "udgt", amount);
        self.set_dgt_total_minted(new_total);
        Ok(amount)
    }

    /// Set balance for specific denomination (used by execution engine)
    pub fn set_balance(&mut self, addr: &str, denom: &str, amount: u128) {
        let mut a = self.get_account(addr);
        a.set_balance(denom, amount);
        self.accounts.insert(addr.to_string(), a.clone());
        let _ = self.storage.set_balances_db(addr, &a.balances);
    }

    /// Increment nonce for an address (used by execution engine)
    pub fn increment_nonce(&mut self, addr: &str) {
        let mut a = self.get_account(addr);
        a.nonce += 1;
        self.accounts.insert(addr.to_string(), a.clone());
        let _ = self.storage.set_nonce_db(addr, a.nonce);
    }

    /// Test helper: set account balance for testing (uses default denomination)
    #[cfg(test)]
    pub fn set_account_balance(&mut self, addr: &str, amount: u128) {
        self.set_balance(addr, "udgt", amount);
    }

    pub fn get_balance(&self, addr: &str, denom: &str) -> u128 {
        self.snapshot_account(addr).balance_of(denom)
    }
}
