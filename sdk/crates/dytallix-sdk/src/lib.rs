//! Keypair, address, transaction, optional network client, faucet client, and
//! keystore support for Dytallix.
//!
//! The SDK models the canonical two-token system used by the Dytallix chain:
//! DGT for governance and delegation, and DRT for rewards. The current public
//! node charges transaction fees in DGT micro-units.
//!
//! Keypair, faucet, transfer, and basic contract lifecycle are available for
//! experimentation on the public testnet. Staking, governance, and some
//! advanced or operator paths are not yet production-complete.

#[cfg(feature = "network")]
pub mod client;
#[cfg(all(feature = "network", feature = "ordinary-http-only"))]
compile_error!("ordinary-http-only cannot be combined with network/TLS; build separately with --no-default-features");
#[cfg(all(
    feature = "strict-local-mldsa65",
    any(
        feature = "compatibility",
        feature = "network",
        feature = "ordinary-http-only"
    )
))]
compile_error!("strict-local-mldsa65 cannot include compatibility or legacy network features");
pub mod error;
#[cfg(feature = "network")]
pub mod faucet;
pub mod keystore;
#[cfg(any(
    feature = "network",
    feature = "ordinary-http-only",
    feature = "strict-local-mldsa65"
))]
pub mod ordinary_client;
pub mod ordinary_v2;
pub mod transaction;

use std::fmt;

pub use dytallix_core::address::DAddr;
pub use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
pub use dytallix_core::signature::verify_mldsa65;
pub use error::SdkError;

/// The two canonical Dytallix tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Token {
    /// Dytallix Governance Token used for governance and delegation.
    DGT,
    /// Dytallix Reward Token used for rewards and burns.
    DRT,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DGT => f.write_str("DGT"),
            Self::DRT => f.write_str("DRT"),
        }
    }
}

/// Token balances for a single Dytallix account.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Balance {
    /// The DGT balance used for governance and delegation.
    pub dgt: u128,
    /// The DRT balance used for rewards and burns.
    pub drt: u128,
}

impl fmt::Display for Balance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  DGT:  {} DGT", self.dgt)?;
        write!(f, "  DRT:  {} DRT", self.drt)
    }
}

/// The current on-chain state for a Dytallix account.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountState {
    /// The canonical account address.
    pub address: DAddr,
    /// The 32-byte public-key hash associated with the account.
    pub pubkey_hash: [u8; 32],
    /// The current token balances.
    pub balance: Balance,
    /// The next transaction nonce for the account.
    pub nonce: u64,
    /// The signing scheme associated with the account key.
    pub key_scheme: KeyScheme,
}

/// A micro-denominated fee estimate split into compute and bandwidth gas.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FeeEstimate {
    /// Compute gas units.
    pub c_gas: u64,
    /// Historical field name retained for SDK compatibility.
    pub c_gas_cost_drt: u128,
    /// Bandwidth gas units.
    pub b_gas: u64,
    /// Historical field name retained for SDK compatibility.
    pub b_gas_cost_drt: u128,
    /// Historical field name retained for SDK compatibility.
    pub total_cost_drt: u128,
}

impl fmt::Display for FeeEstimate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  Fee estimate:")?;
        writeln!(
            f,
            "    Compute (C-Gas):   {} units  {} DGT",
            self.c_gas,
            format_micro_token(self.c_gas_cost_drt)
        )?;
        writeln!(
            f,
            "    Bandwidth (B-Gas): {} units  {} DGT",
            self.b_gas,
            format_micro_token(self.b_gas_cost_drt)
        )?;
        write!(
            f,
            "    Total:             {} DGT",
            format_micro_token(self.total_cost_drt)
        )
    }
}

/// The status of a submitted transaction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransactionStatus {
    /// The transaction has been accepted but not yet confirmed.
    Pending,
    /// The transaction has been confirmed on-chain.
    Confirmed,
    /// The transaction failed with a reason.
    Failed(String),
}

/// A transaction receipt returned by the Dytallix network.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransactionReceipt {
    /// The canonical transaction hash.
    pub hash: String,
    /// The block number containing the transaction.
    pub block: u64,
    /// The transaction execution status.
    pub status: TransactionStatus,
    /// The fee charged for the transaction.
    pub fee: FeeEstimate,
}

fn format_micro_token(value: u128) -> String {
    let whole = value / 1_000_000;
    let fractional = value % 1_000_000;
    if fractional == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fractional:06}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

/// A Dytallix block summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// The block number.
    pub number: u64,
    /// The block hash.
    pub hash: String,
    /// The parent block hash.
    pub parent_hash: String,
    /// The proposer address.
    pub proposer: DAddr,
    /// The slot number.
    pub slot: u64,
    /// The epoch number.
    pub epoch: u64,
    /// The number of transactions in the block.
    pub tx_count: usize,
    /// Total compute gas consumed in the block.
    pub c_gas_used: u64,
    /// Total bandwidth gas consumed in the block.
    pub b_gas_used: u64,
    /// The UNIX timestamp for the block.
    pub timestamp: u64,
}

/// The current chain tip and finalization state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChainStatus {
    /// The latest known block height.
    pub block_height: u64,
    /// The current epoch.
    pub epoch: u64,
    /// The current slot.
    pub slot: u64,
    /// The latest finalized checkpoint identifier.
    pub finalized_checkpoint: String,
}

/// A validator entry in the active validator set.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Validator {
    /// The validator address.
    pub address: DAddr,
    /// The validator stake weight denominated in DGT.
    pub stake_weight: u128,
    /// The validator uptime ratio.
    pub uptime: f64,
    /// The number of slash events recorded for the validator.
    pub slash_count: u32,
}

/// A DGT delegation and its accrued DRT rewards.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Delegation {
    /// The validator receiving the delegation.
    pub validator: DAddr,
    /// The delegated amount in DGT.
    pub amount_dgt: u128,
    /// The unclaimed delegation rewards in DRT.
    pub unclaimed_drt: u128,
}

/// Metadata describing a deployed contract instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractInfo {
    /// The contract address.
    pub address: DAddr,
    /// The deployer address.
    pub deployer: DAddr,
    /// The block number in which the contract was deployed.
    pub deploy_block: u64,
    /// The current contract state root.
    pub state_root: String,
}

/// A block identifier accepted by the SDK client.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlockId {
    /// A block identified by number.
    Number(u64),
    /// A block identified by hash.
    Hash(String),
    /// The latest block.
    Latest,
    /// The latest finalized block.
    Finalized,
}

/// Faucet availability state for an address.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FaucetStatus {
    /// Whether the address may request funds right now.
    pub can_request: bool,
    /// Optional retry window in seconds when the faucet is rate-limited.
    pub retry_after_seconds: Option<u64>,
}

/// A serialized keystore entry containing key material and metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeystoreEntry {
    /// The human-readable key name.
    pub name: String,
    /// The canonical Dytallix address for the key.
    pub address: DAddr,
    /// The raw public key bytes.
    pub public_key: Vec<u8>,
    /// The raw private key bytes.
    pub private_key: Vec<u8>,
    /// The key scheme used by this entry.
    pub scheme: KeyScheme,
    /// The UNIX timestamp at which the key was added.
    pub created_at: u64,
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine as _;

    use crate::keystore::Keystore;
    use crate::transaction::TransactionBuilder;
    use crate::{Balance, DAddr, DytallixKeypair, FeeEstimate, Token};

    #[test]
    fn balance_display() {
        let balance = Balance {
            dgt: 1_000,
            drt: 10_000,
        };
        assert_eq!(balance.to_string(), "  DGT:  1000 DGT\n  DRT:  10000 DRT");
    }

    #[test]
    fn fee_estimate_display() {
        let fee = FeeEstimate {
            c_gas: 21_000,
            c_gas_cost_drt: 42_000,
            b_gas: 512,
            b_gas_cost_drt: 7_000,
            total_cost_drt: 49_000,
        };
        assert_eq!(
            fee.to_string(),
            "  Fee estimate:\n    Compute (C-Gas):   21000 units  0.042 DGT\n    Bandwidth (B-Gas): 512 units  0.007 DGT\n    Total:             0.049 DGT"
        );
    }

    #[test]
    fn transaction_builder_validation() {
        let result = TransactionBuilder::new().build();
        assert!(result.is_err());
    }

    #[test]
    fn sdk_surface_exposes_keypair_and_address() {
        let keypair = DytallixKeypair::generate();
        let address = DAddr::from_public_key(keypair.public_key()).unwrap();
        assert!(address.as_str().starts_with("dytallix1"));
    }

    #[test]
    fn transaction_signing_produces_correct_signature_size() {
        let keypair = DytallixKeypair::generate();
        let address = DAddr::from_public_key(keypair.public_key()).unwrap();
        let transaction = TransactionBuilder::new()
            .from(address.clone())
            .to(address)
            .amount(1, Token::DRT)
            .nonce(0)
            .build()
            .unwrap();

        let signed = transaction.sign(&keypair).unwrap();
        let signature = base64::engine::general_purpose::STANDARD
            .decode(&signed.signature)
            .unwrap();
        assert_eq!(signature.len(), 3_309);
    }

    #[test]
    fn keystore_round_trip() {
        let path = unique_test_keystore_path();
        let keypair = DytallixKeypair::generate();

        let mut keystore = Keystore::new(path.clone()).unwrap();
        keystore.add_keypair(&keypair, "test").unwrap();
        keystore.save().unwrap();

        let reopened = Keystore::open(path.clone()).unwrap();
        let restored = reopened.get_keypair("test").unwrap();

        assert_eq!(restored.public_key(), keypair.public_key());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn keystore_default_path() {
        let path = Keystore::default_path();
        assert!(path.to_string_lossy().ends_with(".dytallix/keystore.json"));
    }

    fn unique_test_keystore_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("dytallix-sdk-keystore-{nanos}.json"))
    }
}
