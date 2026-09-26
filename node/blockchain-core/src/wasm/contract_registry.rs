use super::types::*;
use anyhow::{anyhow, Result};
use blake3::Hasher;
use std::collections::HashMap;

pub trait _ContractStore {
    fn _put_code(&mut self, code: &[u8]) -> Result<CodeHash>;
    fn _get_code(&self, hash: &CodeHash) -> Result<Vec<u8>>;
    fn _create_instance(
        &mut self,
        code_hash: CodeHash,
        creator: Address,
        height: u64,
    ) -> Result<ContractInstance>;
    fn _get_instance(&self, addr: &Address) -> Result<ContractInstance>;
    fn _update_instance(&mut self, instance: &ContractInstance) -> Result<()>;
}

pub struct InMemoryContractStore {
    code: HashMap<Vec<u8>, Vec<u8>>, // hash->code
    instances: HashMap<Vec<u8>, ContractInstance>,
    nonce: u64,
}

impl Default for InMemoryContractStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryContractStore {
    pub fn new() -> Self {
        Self {
            code: HashMap::new(),
            instances: HashMap::new(),
            nonce: 0,
        }
    }
}

fn blake3_hash(data: &[u8]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(data);
    *h.finalize().as_bytes()
}

impl _ContractStore for InMemoryContractStore {
    fn _put_code(&mut self, code: &[u8]) -> Result<CodeHash> {
        let hash = blake3_hash(code);
        self.code.entry(hash.to_vec()).or_insert(code.to_vec());
        Ok(hash)
    }

    fn _get_code(&self, hash: &CodeHash) -> Result<Vec<u8>> {
        self.code
            .get(hash.as_slice())
            .cloned()
            .ok_or_else(|| anyhow!("code not found"))
    }

    fn _create_instance(
        &mut self,
        code_hash: CodeHash,
        creator: Address,
        height: u64,
    ) -> Result<ContractInstance> {
        self.nonce += 1;
        let mut preimage = Vec::new();
        preimage.extend_from_slice(&creator);
        preimage.extend_from_slice(&code_hash);
        preimage.extend_from_slice(&self.nonce.to_le_bytes());
        let address = blake3_hash(&preimage);
        let inst = ContractInstance {
            address,
            code_hash,
            creator,
            deployed_at_height: height,
            last_gas_used: 0,
        };
        self.instances.insert(address.to_vec(), inst.clone());
        Ok(inst)
    }

    fn _get_instance(&self, addr: &Address) -> Result<ContractInstance> {
        self.instances
            .get(addr.as_slice())
            .cloned()
            .ok_or_else(|| anyhow!("instance not found"))
    }

    fn _update_instance(&mut self, instance: &ContractInstance) -> Result<()> {
        self.instances
            .insert(instance.address.to_vec(), instance.clone());
        Ok(())
    }
}
