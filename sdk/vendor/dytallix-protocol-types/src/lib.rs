//! Transaction data, account address formats, and canonical encoding.
mod hash;
pub mod tx;
pub use hash::{canonical_json, sha3_256};
pub use tx::{Msg, Tx};

/// Current signed transaction envelope version. A future change requires protocol activation.
pub const TRANSACTION_FORMAT_VERSION: u32 = 1;
/// Current persisted and public receipt representation.
pub const RECEIPT_FORMAT_VERSION: u32 = 1;

pub mod address;
pub mod units;

/// Recovery transitions over trusted authority facts; no public signing format or route.
pub mod recovery;

/// Local canonical recovery signing codec; no global transaction version change.
pub mod recovery_wire;

/// Explicit recovery sponsor authorization and fee profile codec.
pub mod recovery_sponsor;

/// Ordinary-v2 canonical authorization bytes; paid execution needs its own contract.
pub mod ordinary;

/// Explicit ordinary fee profile format; no implicit activation or prices.
pub mod ordinary_fees;

/// Strict public ordinary RPC views. Reported context is not a light-client proof.
pub mod ordinary_client;
