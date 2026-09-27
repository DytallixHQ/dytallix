//! Governance action data (T6, P01 27 September 2026): the bytes a proposal
//! of the parameter-change or validator-registry class carries. The chain
//! decodes them with bincode 1 (fixed-width little-endian integers, no
//! trailing bytes) and refuses any other encoding. `action_data` shares no
//! code with bincode; the node's tests check that both agree.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CLASS_PARAMETER_CHANGE: u16 = 1;
pub const CLASS_VALIDATOR_REGISTRY: u16 = 2;

/// The fee values governance may set (gas price, per-resource costs and the
/// account creation fee). Every other profile field stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeValues {
    pub gas_price: u64,
    pub transaction_overhead: u64,
    pub receipt_metadata_cost: u64,
    pub wire_byte_cost: u64,
    pub read_byte_cost: u64,
    pub write_byte_cost: u64,
    pub action_costs: [u64; 12],
    pub signature_costs: BTreeMap<String, u64>,
    pub validator_proof_costs: BTreeMap<String, u64>,
    pub governance_action_costs: [u64; 3],
    pub account_creation_fee_udrt: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParameterChange {
    Fees(FeeValues),
    MinSelfBond(u128),
    MaxActive(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryChange {
    /// Approve an operator: validator ID and its owner's native address.
    Add { validator_id: String, owner: String },
    /// Withdraw an approval that no retained validator record uses.
    Remove { validator_id: String },
}

#[derive(Default)]
struct Out(Vec<u8>);
impl Out {
    fn u32(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend(value.to_le_bytes());
    }
    fn u128(&mut self, value: u128) {
        self.0.extend(value.to_le_bytes());
    }
    fn text(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.0.extend(value.as_bytes());
    }
    fn costs(&mut self, costs: &BTreeMap<String, u64>) {
        self.u64(costs.len() as u64);
        for (name, cost) in costs {
            self.text(name);
            self.u64(*cost);
        }
    }
}

impl ParameterChange {
    /// Canonical action data for a `CLASS_PARAMETER_CHANGE` proposal.
    pub fn action_data(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            Self::Fees(values) => {
                out.u32(0);
                for cost in [
                    values.gas_price,
                    values.transaction_overhead,
                    values.receipt_metadata_cost,
                    values.wire_byte_cost,
                    values.read_byte_cost,
                    values.write_byte_cost,
                ]
                .into_iter()
                .chain(values.action_costs)
                {
                    out.u64(cost);
                }
                out.costs(&values.signature_costs);
                out.costs(&values.validator_proof_costs);
                for cost in values.governance_action_costs {
                    out.u64(cost);
                }
                out.u128(values.account_creation_fee_udrt);
            }
            Self::MinSelfBond(value) => {
                out.u32(1);
                out.u128(*value);
            }
            Self::MaxActive(value) => {
                out.u32(2);
                out.u64(*value);
            }
        }
        out.0
    }
}

impl RegistryChange {
    /// Canonical action data for a `CLASS_VALIDATOR_REGISTRY` proposal.
    pub fn action_data(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            Self::Add {
                validator_id,
                owner,
            } => {
                out.u32(0);
                out.text(validator_id);
                out.text(owner);
            }
            Self::Remove { validator_id } => {
                out.u32(1);
                out.text(validator_id);
            }
        }
        out.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_changes_have_fixed_bytes() {
        assert_eq!(
            ParameterChange::MaxActive(16).action_data(),
            [&[2, 0, 0, 0][..], &16u64.to_le_bytes()].concat()
        );
        assert_eq!(
            RegistryChange::Remove {
                validator_id: "v1".into()
            }
            .action_data(),
            [&[1, 0, 0, 0][..], &2u64.to_le_bytes(), b"v1"].concat()
        );
    }
}
