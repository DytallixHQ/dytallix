use crate::types::{Address, BlockNumber};
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Encode, Decode, Serialize, Deserialize)]
pub enum AlgorithmCapability {
    Signature,
    KeyEncapsulation,
    Hash,
    Randomness,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum AlgorithmStatus {
    Active,
    Deprecated {
        sunset_block: BlockNumber,
        reason: String,
    },
    Revoked {
        reason: String,
    },
    Suspended {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct AlgorithmRecord {
    pub id: String,
    pub family: String,
    pub variant: String,
    pub standard_reference: String,
    pub security_bits: u16,
    pub capabilities: BTreeSet<AlgorithmCapability>,
    pub status: AlgorithmStatus,
    pub added_by: Address,
    pub updated_at: BlockNumber,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum RegistryEvent {
    Registered {
        algorithm_id: String,
    },
    Deprecated {
        algorithm_id: String,
        sunset_block: BlockNumber,
    },
    Revoked {
        algorithm_id: String,
    },
    CircuitBreakerTriggered {
        reason: String,
    },
    CircuitBreakerCleared,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("algorithm already exists")]
    DuplicateAlgorithm,
    #[error("algorithm not found")]
    AlgorithmNotFound,
    #[error("circuit breaker is active")]
    CircuitBreakerActive,
}

pub type RegistryResult<T> = Result<T, RegistryError>;

#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct AlgorithmRegistry {
    pub owner: Address,
    pub guardians: BTreeSet<Address>,
    pub algorithms: BTreeMap<String, AlgorithmRecord>,
    pub circuit_breaker_active: bool,
    pub pause_reason: Option<String>,
    pub events: Vec<RegistryEvent>,
}

impl AlgorithmRegistry {
    pub fn new(owner: Address) -> Self {
        Self {
            owner,
            guardians: BTreeSet::new(),
            algorithms: BTreeMap::new(),
            circuit_breaker_active: false,
            pause_reason: None,
            events: Vec::new(),
        }
    }

    pub fn add_guardian(&mut self, caller: &Address, guardian: Address) -> RegistryResult<()> {
        self.ensure_owner(caller)?;
        self.guardians.insert(guardian);
        Ok(())
    }

    pub fn register_algorithm(
        &mut self,
        caller: &Address,
        mut record: AlgorithmRecord,
    ) -> RegistryResult<()> {
        self.ensure_admin(caller)?;
        if self.circuit_breaker_active {
            return Err(RegistryError::CircuitBreakerActive);
        }
        if self.algorithms.contains_key(&record.id) {
            return Err(RegistryError::DuplicateAlgorithm);
        }

        record.added_by = caller.clone();
        self.events.push(RegistryEvent::Registered {
            algorithm_id: record.id.clone(),
        });
        self.algorithms.insert(record.id.clone(), record);
        Ok(())
    }

    pub fn deprecate_algorithm(
        &mut self,
        caller: &Address,
        algorithm_id: &str,
        sunset_block: BlockNumber,
        reason: String,
        current_block: BlockNumber,
    ) -> RegistryResult<()> {
        self.ensure_admin(caller)?;
        let record = self
            .algorithms
            .get_mut(algorithm_id)
            .ok_or(RegistryError::AlgorithmNotFound)?;
        record.status = AlgorithmStatus::Deprecated {
            sunset_block,
            reason,
        };
        record.updated_at = current_block;
        self.events.push(RegistryEvent::Deprecated {
            algorithm_id: algorithm_id.to_string(),
            sunset_block,
        });
        Ok(())
    }

    pub fn revoke_algorithm(
        &mut self,
        caller: &Address,
        algorithm_id: &str,
        reason: String,
        current_block: BlockNumber,
    ) -> RegistryResult<()> {
        self.ensure_admin(caller)?;
        let record = self
            .algorithms
            .get_mut(algorithm_id)
            .ok_or(RegistryError::AlgorithmNotFound)?;
        record.status = AlgorithmStatus::Revoked { reason };
        record.updated_at = current_block;
        self.events.push(RegistryEvent::Revoked {
            algorithm_id: algorithm_id.to_string(),
        });
        Ok(())
    }

    pub fn trigger_circuit_breaker(
        &mut self,
        caller: &Address,
        reason: String,
    ) -> RegistryResult<()> {
        self.ensure_emergency_authority(caller)?;
        self.circuit_breaker_active = true;
        self.pause_reason = Some(reason.clone());
        self.events
            .push(RegistryEvent::CircuitBreakerTriggered { reason });
        Ok(())
    }

    pub fn clear_circuit_breaker(&mut self, caller: &Address) -> RegistryResult<()> {
        self.ensure_owner(caller)?;
        self.circuit_breaker_active = false;
        self.pause_reason = None;
        self.events.push(RegistryEvent::CircuitBreakerCleared);
        Ok(())
    }

    pub fn can_use(
        &self,
        algorithm_id: &str,
        capability: &AlgorithmCapability,
        current_block: BlockNumber,
    ) -> bool {
        if self.circuit_breaker_active {
            return false;
        }

        let Some(record) = self.algorithms.get(algorithm_id) else {
            return false;
        };
        if !record.capabilities.contains(capability) {
            return false;
        }

        match &record.status {
            AlgorithmStatus::Active => true,
            AlgorithmStatus::Deprecated { sunset_block, .. } => current_block < *sunset_block,
            AlgorithmStatus::Revoked { .. } | AlgorithmStatus::Suspended { .. } => false,
        }
    }

    pub fn active_algorithms(&self, current_block: BlockNumber) -> Vec<&AlgorithmRecord> {
        self.algorithms
            .values()
            .filter(|record| {
                self.can_use(&record.id, &AlgorithmCapability::Signature, current_block)
                    || self.can_use(
                        &record.id,
                        &AlgorithmCapability::KeyEncapsulation,
                        current_block,
                    )
                    || self.can_use(&record.id, &AlgorithmCapability::Hash, current_block)
                    || self.can_use(&record.id, &AlgorithmCapability::Randomness, current_block)
            })
            .collect()
    }

    fn ensure_owner(&self, caller: &Address) -> RegistryResult<()> {
        if caller == &self.owner {
            Ok(())
        } else {
            Err(RegistryError::Unauthorized)
        }
    }

    fn ensure_admin(&self, caller: &Address) -> RegistryResult<()> {
        if caller == &self.owner || self.guardians.contains(caller) {
            Ok(())
        } else {
            Err(RegistryError::Unauthorized)
        }
    }

    fn ensure_emergency_authority(&self, caller: &Address) -> RegistryResult<()> {
        self.ensure_admin(caller)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_tracks_algorithm_lifecycle() {
        let owner = "owner".to_string();
        let mut registry = AlgorithmRegistry::new(owner.clone());
        registry.add_guardian(&owner, "guardian".into()).unwrap();

        let mut capabilities = BTreeSet::new();
        capabilities.insert(AlgorithmCapability::Signature);
        capabilities.insert(AlgorithmCapability::KeyEncapsulation);

        registry
            .register_algorithm(
                &"guardian".to_string(),
                AlgorithmRecord {
                    id: "ml-dsa-65".into(),
                    family: "ML-DSA".into(),
                    variant: "65".into(),
                    standard_reference: "FIPS 204".into(),
                    security_bits: 192,
                    capabilities,
                    status: AlgorithmStatus::Active,
                    added_by: String::new(),
                    updated_at: 10,
                    notes: Some("default signing algorithm".into()),
                },
            )
            .unwrap();

        assert!(registry.can_use("ml-dsa-65", &AlgorithmCapability::Signature, 10));

        registry
            .deprecate_algorithm(
                &owner,
                "ml-dsa-65",
                1_000,
                "move to newer profile".into(),
                20,
            )
            .unwrap();
        assert!(registry.can_use("ml-dsa-65", &AlgorithmCapability::Signature, 100));
        assert!(!registry.can_use("ml-dsa-65", &AlgorithmCapability::Signature, 1_000));
    }

    #[test]
    fn circuit_breaker_disables_all_usage() {
        let owner = "owner".to_string();
        let mut registry = AlgorithmRegistry::new(owner.clone());

        let mut capabilities = BTreeSet::new();
        capabilities.insert(AlgorithmCapability::Hash);

        registry
            .register_algorithm(
                &owner,
                AlgorithmRecord {
                    id: "shake256".into(),
                    family: "SHAKE".into(),
                    variant: "256".into(),
                    standard_reference: "FIPS 202".into(),
                    security_bits: 256,
                    capabilities,
                    status: AlgorithmStatus::Active,
                    added_by: String::new(),
                    updated_at: 1,
                    notes: None,
                },
            )
            .unwrap();

        registry
            .trigger_circuit_breaker(&owner, "investigating upstream advisory".into())
            .unwrap();
        assert!(!registry.can_use("shake256", &AlgorithmCapability::Hash, 2));

        registry.clear_circuit_breaker(&owner).unwrap();
        assert!(registry.can_use("shake256", &AlgorithmCapability::Hash, 3));
    }
}
