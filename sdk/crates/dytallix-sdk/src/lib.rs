//! Keypairs, addresses, an encrypted keystore and the consensus chain's
//! transactions for Dytallix:
//! - ordinary-v2 and ordinary-v3 building and signing;
//! - recovery transactions;
//! - with `comet-rpc`, a Comet JSON-RPC client. It reaches a node over the
//!   post-quantum client channel or plain loopback HTTP; there is no TLS
//!   (E04 gap 19).
//!
//! The SDK models the canonical two-token system: DGT for governance and
//! delegation, and DRT for fees and rewards.

#[cfg(all(feature = "comet-rpc", feature = "ordinary-http-only"))]
compile_error!("ordinary-http-only cannot be combined with comet-rpc; build separately with --no-default-features");
#[cfg(all(
    feature = "strict-local-mldsa65",
    any(
        feature = "compatibility",
        feature = "comet-rpc",
        feature = "ordinary-http-only"
    )
))]
compile_error!("strict-local-mldsa65 cannot include compatibility or comet-rpc");
pub mod error;
pub mod keystore;
#[cfg(any(
    feature = "comet-rpc",
    feature = "ordinary-http-only",
    feature = "strict-local-mldsa65"
))]
pub mod ordinary_client;
pub mod ordinary_v2;
pub mod ordinary_v3;
/// Recovery transactions (E04 gap 17).
pub mod recovery;
pub mod transaction;
/// How requests reach a node: loopback HTTP or the client channel (E04 gap 19).
#[cfg(any(
    feature = "comet-rpc",
    feature = "ordinary-http-only",
    feature = "strict-local-mldsa65"
))]
pub mod transport;

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

/// A keystore entry's public metadata. Version 2 keystores keep the private
/// key encrypted and bind this metadata to it (E04 gap 16).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeystoreEntry {
    /// The human-readable key name.
    pub name: String,
    /// The canonical Dytallix address for the key.
    pub address: DAddr,
    /// The raw public key bytes.
    pub public_key: Vec<u8>,
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
    use crate::transaction::{Message, TransactionBuilder};
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

    /// An amount and a payload together are refused, never sent as a
    /// transfer with the payload dropped (E04 gap 8, K3).
    #[test]
    fn builder_refuses_an_amount_with_a_payload() {
        let keypair = DytallixKeypair::generate();
        let address = DAddr::from_public_key(keypair.public_key()).unwrap();
        let builder = || {
            TransactionBuilder::new()
                .from(address.clone())
                .to(address.clone())
                .nonce(0)
        };
        let both = builder()
            .amount(5, Token::DGT)
            .data(b"stake:delegate:v:5".to_vec())
            .build();
        assert!(both.is_err());
        let data_only = builder().data(b"stake:claim".to_vec()).build().unwrap();
        assert!(matches!(data_only.msgs[..], [Message::Data { .. }]));
        let amount_only = builder().amount(5, Token::DGT).build().unwrap();
        assert!(matches!(amount_only.msgs[..], [Message::Send { .. }]));
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

        let mut keystore = Keystore::create(path.clone(), b"round trip").unwrap();
        keystore.add_keypair(&keypair, "test").unwrap();
        keystore.save().unwrap();

        let mut reopened = Keystore::open(path.clone()).unwrap();
        reopened.unlock(b"round trip").unwrap();
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
