use serde::{Deserialize, Serialize};

pub type Address = [u8; 32];
pub type CodeHash = [u8; 32];

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContractInstance {
    pub address: Address,
    pub code_hash: CodeHash,
    pub creator: Address,
    pub deployed_at_height: u64,
    pub last_gas_used: u64,
}
