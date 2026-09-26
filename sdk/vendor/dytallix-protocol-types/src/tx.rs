use crate::{canonical_json, sha3_256};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// Helper module to (de)serialize u128 as string for canonical on-wire format.
mod as_str_u128 {
    use serde::{self, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s: String = Deserialize::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// Reward amounts use canonical nonnegative decimal strings only.
mod reward_u128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(serde::de::Error::custom(
                "Reward amount must be a canonical decimal string",
            ));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    Send {
        from: String,
        to: String,
        denom: String,
        #[serde(with = "as_str_u128")]
        amount: u128,
    },
    Data {
        from: String,
        data: String, // JSON or arbitrary string data to anchor on-chain
    },
    DmsRegister {
        from: String,
        beneficiary: String,
        #[serde(with = "as_str_u128")]
        period: u128,
    },
    DmsPing {
        from: String,
    },
    DmsClaim {
        from: String,
        owner: String,
    },
    #[serde(rename = "reward_bond")]
    RewardBond {
        from: String,
        validator: String,
        #[serde(with = "reward_u128")]
        amount_udgt: u128,
    },
    #[serde(rename = "reward_begin_unbond")]
    RewardBeginUnbond {
        from: String,
        validator: String,
        #[serde(with = "reward_u128")]
        amount_udgt: u128,
    },
    #[serde(rename = "reward_claim")]
    RewardClaim {
        from: String,
    },
    #[serde(rename = "validator_register")]
    ValidatorRegister {
        from: String,
        validator: String,
        consensus_pubkey: String,
        proof: String,
        expires_at_height: u64,
        #[serde(with = "reward_u128")]
        amount_udgt: u128,
    },
    #[serde(rename = "validator_rotate_key")]
    ValidatorRotateKey {
        from: String,
        validator: String,
        consensus_pubkey: String,
        proof: String,
        expires_at_height: u64,
    },
    #[serde(rename = "validator_exit")]
    ValidatorExit {
        from: String,
        validator: String,
    },
    #[serde(rename = "validator_withdraw")]
    ValidatorWithdraw {
        from: String,
        unbond_id: String,
    },
}

impl Msg {
    pub fn validate(&self) -> Result<()> {
        match self {
            Msg::Send {
                from,
                to,
                denom,
                amount,
            } => {
                eprintln!(
                    "[DEBUG msg validate] amount={}, denom='{}', from='{}', to='{}'",
                    amount, denom, from, to
                );
                if *amount == 0 {
                    return Err(anyhow!("amount cannot be zero"));
                }
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
                if to.is_empty() {
                    return Err(anyhow!("to address cannot be empty"));
                }
                // Accept both micro-denominations (udgt, udrt) and whole tokens (DGT, DRT)
                let up = denom.to_ascii_uppercase();
                eprintln!("[DEBUG msg validate] denom uppercase: '{}', checking if DGT, DRT, UDGT, or UDRT", up);
                if up != "DGT" && up != "DRT" && up != "UDGT" && up != "UDRT" {
                    eprintln!("[DEBUG msg validate] ❌ DENOM VALIDATION FAILED: got '{}', expected DGT, DRT, udgt, or udrt", denom);
                    return Err(anyhow!(
                        "unsupported denom: {}; valid: DGT, DRT, udgt, udrt",
                        denom
                    ));
                }
                eprintln!("[DEBUG msg validate] ✅ Message validation passed");
            }
            Msg::Data { from, data } => {
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
                if data.is_empty() {
                    return Err(anyhow!("data cannot be empty"));
                }
                // Limit data size to 1MB
                if data.len() > 1_000_000 {
                    return Err(anyhow!("data too large: max 1MB"));
                }
            }
            Msg::DmsRegister {
                from,
                beneficiary,
                period,
            } => {
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
                if beneficiary.is_empty() {
                    return Err(anyhow!("beneficiary address cannot be empty"));
                }
                if *period == 0 {
                    return Err(anyhow!("period cannot be zero"));
                }
            }
            Msg::RewardBond {
                from,
                validator,
                amount_udgt,
            }
            | Msg::RewardBeginUnbond {
                from,
                validator,
                amount_udgt,
            } => {
                if from.is_empty() || validator.is_empty() {
                    return Err(anyhow!("Reward sender and validator cannot be empty"));
                }
                if *amount_udgt == 0 {
                    return Err(anyhow!("Reward stake amount cannot be zero"));
                }
            }
            Msg::ValidatorRegister {
                from,
                validator,
                consensus_pubkey,
                proof,
                expires_at_height,
                amount_udgt,
            } => {
                validate_validator_key_message(
                    from,
                    validator,
                    consensus_pubkey,
                    proof,
                    *expires_at_height,
                )?;
                if *amount_udgt == 0 {
                    return Err(anyhow!("Validator self-bond cannot be zero"));
                }
            }
            Msg::ValidatorRotateKey {
                from,
                validator,
                consensus_pubkey,
                proof,
                expires_at_height,
            } => {
                validate_validator_key_message(
                    from,
                    validator,
                    consensus_pubkey,
                    proof,
                    *expires_at_height,
                )?;
            }
            Msg::ValidatorExit { from, validator } => {
                validate_validator_identity(from, validator)?;
            }
            Msg::ValidatorWithdraw { from, unbond_id } => {
                validate_validator_identity(from, unbond_id)?;
            }
            Msg::RewardClaim { from } => {
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
            }
            Msg::DmsPing { from } => {
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
            }
            Msg::DmsClaim { from, owner } => {
                if from.is_empty() {
                    return Err(anyhow!("from address cannot be empty"));
                }
                if owner.is_empty() {
                    return Err(anyhow!("owner address cannot be empty"));
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn sender(&self) -> &str {
        match self {
            Msg::Send { from, .. } => from,
            Msg::Data { from, .. } => from,
            Msg::DmsRegister { from, .. } => from,
            Msg::DmsPing { from, .. } => from,
            Msg::DmsClaim { from, .. }
            | Msg::RewardBond { from, .. }
            | Msg::RewardBeginUnbond { from, .. }
            | Msg::RewardClaim { from }
            | Msg::ValidatorRegister { from, .. }
            | Msg::ValidatorRotateKey { from, .. }
            | Msg::ValidatorExit { from, .. }
            | Msg::ValidatorWithdraw { from, .. } => from,
        }
    }
}

fn validate_validator_identity(from: &str, id: &str) -> Result<()> {
    if from.is_empty() || from.len() > 256 || id.is_empty() || id.len() > 256 {
        return Err(anyhow!(
            "Validator sender and identifier must contain 1 to 256 bytes"
        ));
    }
    Ok(())
}

fn validate_validator_key_message(
    from: &str,
    validator: &str,
    key: &str,
    proof: &str,
    expiry: u64,
) -> Result<()> {
    validate_validator_identity(from, validator)?;
    if key.is_empty() || key.len() > 2604 || proof.is_empty() || proof.len() > 4412 || expiry == 0 {
        return Err(anyhow!("Validator key proof size or expiry invalid"));
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tx {
    pub chain_id: String,
    pub nonce: u64,
    pub msgs: Vec<Msg>,
    #[serde(with = "as_str_u128")]
    pub fee: u128,
    pub memo: String,
}

impl Tx {
    pub fn new(
        chain_id: impl Into<String>,
        nonce: u64,
        msgs: Vec<Msg>,
        fee: u128,
        memo: impl Into<String>,
    ) -> Result<Self> {
        if msgs.is_empty() {
            return Err(anyhow!("transaction must contain at least one message"));
        }
        for m in &msgs {
            m.validate()?;
        }
        if fee == 0 {
            return Err(anyhow!("fee cannot be zero"));
        }
        let chain_id = chain_id.into();
        if chain_id.is_empty() {
            return Err(anyhow!("chain_id cannot be empty"));
        }
        Ok(Self {
            chain_id,
            nonce,
            msgs,
            fee,
            memo: memo.into(),
        })
    }

    pub fn validate(&self, expected_chain_id: &str) -> Result<()> {
        eprintln!("[DEBUG validate] Comparing chain IDs:");
        eprintln!(
            "[DEBUG validate]   Expected: '{}' (len: {})",
            expected_chain_id,
            expected_chain_id.len()
        );
        eprintln!(
            "[DEBUG validate]   Got:      '{}' (len: {})",
            self.chain_id,
            self.chain_id.len()
        );
        eprintln!(
            "[DEBUG validate]   Match: {}",
            self.chain_id == expected_chain_id
        );

        if self.chain_id != expected_chain_id {
            return Err(anyhow!(
                "invalid chain_id: expected {}, got {}",
                expected_chain_id,
                self.chain_id
            ));
        }
        if self.msgs.is_empty() {
            return Err(anyhow!("transaction must contain at least one message"));
        }
        if self.fee == 0 {
            return Err(anyhow!("fee cannot be zero"));
        }
        for msg in &self.msgs {
            msg.validate()?;
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<[u8; 32]> {
        let bytes = canonical_json(self)?;
        Ok(sha3_256(&bytes))
    }

    pub fn tx_hash(&self) -> Result<String> {
        let hash = self.canonical_hash()?;
        Ok(format!("0x{}", hex::encode(hash)))
    }
}

#[cfg(test)]
mod reward_wire_tests {
    use super::*;

    #[test]
    fn reward_amounts_round_trip_at_full_width() {
        for kind in ["reward_bond", "reward_begin_unbond"] {
            let value = serde_json::json!({"type":kind,"from":"owner","validator":"validator","amount_udgt":u128::MAX.to_string()});
            let message: Msg = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&message).unwrap(), value);
        }
        let claim = serde_json::json!({"type":"reward_claim","from":"owner"});
        let message: Msg = serde_json::from_value(claim.clone()).unwrap();
        assert_eq!(serde_json::to_value(&message).unwrap(), claim);
    }

    #[test]
    fn reward_amounts_reject_noncanonical_and_overflow_inputs() {
        for amount in [
            "",
            "-1",
            "+1",
            "01",
            "1.0",
            "1e6",
            " 1",
            "340282366920938463463374607431768211456",
        ] {
            for kind in ["reward_bond", "reward_begin_unbond"] {
                let value = serde_json::json!({"type":kind,"from":"owner","validator":"validator","amount_udgt":amount});
                assert!(
                    serde_json::from_value::<Msg>(value).is_err(),
                    "{kind}: {amount}"
                );
            }
        }
        let numeric = serde_json::json!({"type":"reward_bond","from":"owner","validator":"validator","amount_udgt":1});
        assert!(serde_json::from_value::<Msg>(numeric).is_err());
    }
}

#[cfg(test)]
mod reward_validation_tests {
    use super::*;
    #[test]
    fn reward_messages_validate_owner_validator_and_positive_stake() {
        for message in [
            Msg::RewardBond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 1,
            },
            Msg::RewardBeginUnbond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 1,
            },
            Msg::RewardClaim {
                from: "owner".into(),
            },
        ] {
            message.validate().unwrap();
            assert_eq!(message.sender(), "owner");
        }
        for message in [
            Msg::RewardBond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 0,
            },
            Msg::RewardBeginUnbond {
                from: "owner".into(),
                validator: "".into(),
                amount_udgt: 1,
            },
            Msg::RewardClaim { from: "".into() },
        ] {
            assert!(message.validate().is_err());
        }
    }
}

#[cfg(test)]
mod validator_wire_tests {
    use super::*;
    #[test]
    fn validator_registration_requires_canonical_full_width_amount_and_bounded_proof() {
        let valid = serde_json::json!({"type":"validator_register","from":"owner","validator":"validator","consensus_pubkey":"k".repeat(2604),"proof":"s".repeat(4412),"expires_at_height":99,"amount_udgt":u128::MAX.to_string()});
        let message: Msg = serde_json::from_value(valid.clone()).unwrap();
        message.validate().unwrap();
        assert_eq!(serde_json::to_value(message).unwrap(), valid);
        for amount in [
            serde_json::json!(1),
            serde_json::json!("01"),
            serde_json::json!("-1"),
            serde_json::json!("340282366920938463463374607431768211456"),
        ] {
            let mut value = valid.clone();
            value["amount_udgt"] = amount;
            assert!(serde_json::from_value::<Msg>(value).is_err());
        }
        for (field, value) in [
            ("amount_udgt", serde_json::json!("0")),
            ("expires_at_height", serde_json::json!(0)),
            ("consensus_pubkey", serde_json::json!("k".repeat(2605))),
            ("proof", serde_json::json!("s".repeat(4413))),
            ("validator", serde_json::json!("v".repeat(257))),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<Msg>(invalid)
                .unwrap()
                .validate()
                .is_err());
        }
    }
}
