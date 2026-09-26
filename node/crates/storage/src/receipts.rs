use super::tx::Transaction;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

// Receipt format version for backward compatibility
pub use dytallix_protocol_types::RECEIPT_FORMAT_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TxStatus {
    Pending,
    Success,
    Failed,
}

fn as_str<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

fn de_u128<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
    struct U128Visitor;
    impl<'de> Visitor<'de> for U128Visitor {
        type Value = u128;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "a string or number representing u128")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse::<u128>()
                .map_err(|_| E::custom("invalid u128 string"))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v as u128)
        }
        fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
            Ok(v)
        }
    }
    d.deserialize_any(U128Visitor)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    pub receipt_version: u32, // RECEIPT_FORMAT_VERSION for versioning
    pub tx_hash: String,
    pub status: TxStatus,
    pub block_height: Option<u64>,
    pub index: Option<u32>,
    pub from: String,
    pub to: String,
    #[serde(serialize_with = "as_str", deserialize_with = "de_u128")]
    pub amount: u128,
    #[serde(serialize_with = "as_str", deserialize_with = "de_u128")]
    pub fee: u128,
    pub nonce: u64,
    pub error: Option<String>,
    // Gas accounting fields
    pub gas_used: u64,   // Actual gas consumed during execution
    pub gas_limit: u64,  // Gas limit from the transaction
    pub gas_price: u64,  // Gas price from the transaction (in datt)
    pub gas_refund: u64, // Gas refund (always 0 for now, stub for future)
    pub success: bool,   // Whether the transaction succeeded
}

impl TxReceipt {
    pub fn pending(tx: &Transaction) -> Self {
        Self {
            receipt_version: RECEIPT_FORMAT_VERSION,
            tx_hash: tx.hash.clone(),
            status: TxStatus::Pending,
            block_height: None,
            index: None,
            from: tx.from.clone(),
            to: tx.to.clone(),
            amount: tx.amount,
            fee: tx.fee,
            nonce: tx.nonce,
            error: None,
            gas_used: 0,
            gas_limit: 0,  // Will be set from SignedTx when available
            gas_price: 0,  // Will be set from SignedTx when available
            gas_refund: 0, // Always 0 for now
            success: false,
        }
    }

    /// Create a receipt for a successful transaction
    pub fn success(
        tx: &Transaction,
        gas_used: u64,
        gas_limit: u64,
        gas_price: u64,
        block_height: u64,
        index: u32,
    ) -> Self {
        Self {
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
            gas_refund: 0, // Always 0 for now
            success: true,
        }
    }

    /// Create a receipt for a failed transaction
    pub fn failed(
        tx: &Transaction,
        gas_used: u64,
        gas_limit: u64,
        gas_price: u64,
        error: String,
        block_height: u64,
        index: u32,
    ) -> Self {
        Self {
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
            gas_refund: 0, // Always 0 for now
            success: false,
        }
    }

    /// Calculate the total fee charged in datt (gas_limit * gas_price)
    /// Note: In case of failure, full gas_limit is charged as per specification
    pub fn fee_charged_datt(&self) -> u64 {
        self.gas_limit.saturating_mul(self.gas_price)
    }
}
