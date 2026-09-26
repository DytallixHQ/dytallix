use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Address type for contracts and accounts
pub type Address = String;

/// Amount type for token values
pub type Amount = u128;

/// Block number type
pub type BlockNumber = u64;

/// Transaction hash type
pub type Hash = [u8; 32];

/// Gas amount type
pub type Gas = u64;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct AccountInfo {
    pub address: Address,
    pub balance: Amount,
    pub nonce: u64,
    pub code_hash: Option<Hash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub hash: Hash,
    pub from: Address,
    pub to: Option<Address>,
    pub value: Amount,
    pub gas_limit: Gas,
    pub gas_price: Amount,
    pub data: Vec<u8>,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct BlockInfo {
    pub number: BlockNumber,
    pub hash: Hash,
    pub parent_hash: Hash,
    pub timestamp: u64,
    pub gas_limit: Gas,
    pub gas_used: Gas,
    pub transactions: Vec<Hash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct ContractMetadata {
    pub name: String,
    pub version: String,
    pub author: Address,
    pub description: String,
    pub abi: Vec<u8>,
    pub source_code_hash: Option<Hash>,
}
