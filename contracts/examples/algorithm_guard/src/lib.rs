use dytallix_contracts::algorithm_registry::{
    AlgorithmCapability, AlgorithmRecord, AlgorithmRegistry, AlgorithmStatus, RegistryError,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlgorithmAttestation {
    pub sender: String,
    pub algorithm_id: String,
    pub payload_hash: Vec<u8>,
    pub block: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgorithmGuardContract {
    pub registry: AlgorithmRegistry,
    pub required_capability: AlgorithmCapability,
    pub attestations: BTreeMap<String, AlgorithmAttestation>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AlgorithmGuardError {
    #[error("algorithm is not currently approved for this contract")]
    AlgorithmNotApproved,
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

impl AlgorithmGuardContract {
    pub fn new(owner: impl Into<String>, required_capability: AlgorithmCapability) -> Self {
        Self {
            registry: AlgorithmRegistry::new(owner.into()),
            required_capability,
            attestations: BTreeMap::new(),
        }
    }

    pub fn register_reference_algorithm(
        &mut self,
        caller: &str,
        id: &str,
        family: &str,
        variant: &str,
        standard_reference: &str,
        capability: AlgorithmCapability,
        block: u64,
    ) -> Result<(), AlgorithmGuardError> {
        let mut capabilities = BTreeSet::new();
        capabilities.insert(capability);
        self.registry.register_algorithm(
            &caller.to_string(),
            AlgorithmRecord {
                id: id.to_string(),
                family: family.to_string(),
                variant: variant.to_string(),
                standard_reference: standard_reference.to_string(),
                security_bits: 192,
                capabilities,
                status: AlgorithmStatus::Active,
                added_by: String::new(),
                updated_at: block,
                notes: None,
            },
        )?;
        Ok(())
    }

    pub fn submit_attestation(
        &mut self,
        sender: impl Into<String>,
        algorithm_id: &str,
        payload_hash: Vec<u8>,
        block: u64,
    ) -> Result<(), AlgorithmGuardError> {
        if !self
            .registry
            .can_use(algorithm_id, &self.required_capability, block)
        {
            return Err(AlgorithmGuardError::AlgorithmNotApproved);
        }

        let sender = sender.into();
        self.attestations.insert(
            sender.clone(),
            AlgorithmAttestation {
                sender,
                algorithm_id: algorithm_id.to_string(),
                payload_hash,
                block,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_algorithms_can_submit_attestations() {
        let mut contract = AlgorithmGuardContract::new("owner", AlgorithmCapability::Signature);
        contract
            .register_reference_algorithm(
                "owner",
                "ml-dsa-65",
                "ML-DSA",
                "65",
                "FIPS 204",
                AlgorithmCapability::Signature,
                10,
            )
            .unwrap();

        contract
            .submit_attestation("alice", "ml-dsa-65", vec![1, 2, 3], 11)
            .unwrap();
        assert!(contract.attestations.contains_key("alice"));
    }

    #[test]
    fn circuit_breaker_blocks_new_attestations() {
        let mut contract = AlgorithmGuardContract::new("owner", AlgorithmCapability::Signature);
        contract
            .register_reference_algorithm(
                "owner",
                "ml-dsa-65",
                "ML-DSA",
                "65",
                "FIPS 204",
                AlgorithmCapability::Signature,
                10,
            )
            .unwrap();

        contract
            .registry
            .trigger_circuit_breaker(&"owner".to_string(), "investigating issue".into())
            .unwrap();

        let err = contract
            .submit_attestation("alice", "ml-dsa-65", vec![1, 2, 3], 11)
            .unwrap_err();
        assert_eq!(err, AlgorithmGuardError::AlgorithmNotApproved);
    }
}
