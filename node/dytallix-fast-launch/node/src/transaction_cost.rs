//! Shared selected-node fee and reservation arithmetic. No mainnet fee policy.
use crate::storage::tx::{Transaction, TxMessage};
use dytallix_protocol_types::units::{DGT_BASE_DENOM, DGT_SCALE, DRT_BASE_DENOM, DRT_SCALE};
use std::collections::BTreeMap;

pub fn effective_gas(tx: &Transaction) -> Result<(u64, u64), String> {
    if tx.gas_limit > 0 && tx.gas_price > 0 {
        Ok((tx.gas_limit, tx.gas_price))
    } else {
        Ok((
            u64::try_from(tx.fee).map_err(|_| "Legacy fee exceeds u64 gas limit")?,
            1,
        ))
    }
}
pub fn upfront_fee(tx: &Transaction) -> Result<u128, String> {
    let (limit, price) = effective_gas(tx)?;
    Ok(u128::from(limit) * u128::from(price))
}
/// Public whole-token aliases use the approved six-decimal scale. Native balances
/// are not rescaled. Stored transaction messages always use canonical base units.
pub fn normalize_send(denom: &str, amount: u128) -> Result<(String, u128), String> {
    match denom.to_ascii_lowercase().as_str() {
        "dgt" => Ok((
            DGT_BASE_DENOM.into(),
            DGT_SCALE
                .whole_tokens_to_base_units(amount)
                .map_err(|_| "DGT input exceeds u128 base units")?,
        )),
        "drt" => Ok((
            DRT_BASE_DENOM.into(),
            DRT_SCALE
                .whole_tokens_to_base_units(amount)
                .map_err(|_| "DRT input exceeds u128 base units")?,
        )),
        DGT_BASE_DENOM => Ok((DGT_BASE_DENOM.into(), amount)),
        DRT_BASE_DENOM => Ok((DRT_BASE_DENOM.into(), amount)),
        _ => Err("Unsupported native denomination".into()),
    }
}
/// Reserve the fee once in UDRT and the peak required sender balance for each token.
/// A self-transfer needs available funds but does not increase cumulative outflow.
/// Pending transactions reserve independently; future incoming credits are not assumed.
pub fn required_balances(tx: &Transaction) -> Result<BTreeMap<String, u128>, String> {
    let fee = upfront_fee(tx)?;
    let mut spent = BTreeMap::from([("udrt".to_string(), fee)]);
    let mut required = spent.clone();
    let mut send = |from: &str, to: &str, denom: &str, amount: u128| -> Result<(), String> {
        if from != tx.from {
            return Err("Message sender differs from transaction sender".into());
        }
        if !matches!(denom, "udgt" | "udrt") {
            return Err("Stored transfer denomination must be udgt or udrt".into());
        }
        let used = spent.get(denom).copied().unwrap_or(0u128);
        let peak = used
            .checked_add(amount)
            .ok_or("Required balance exceeds u128")?;
        required
            .entry(denom.into())
            .and_modify(|v| *v = (*v).max(peak))
            .or_insert(peak);
        if from != to {
            spent.insert(denom.into(), peak);
        }
        Ok(())
    };
    if let Some(messages) = &tx.messages {
        for message in messages {
            match message {
                TxMessage::Send {
                    from,
                    to,
                    denom,
                    amount,
                } => send(from, to, denom, *amount)?,
                // Bonded funds leave the liquid balance even when the validator is the sender.
                TxMessage::RewardBond {
                    from, amount_udgt, ..
                }
                | TxMessage::ValidatorRegister {
                    from, amount_udgt, ..
                } => {
                    send(from, "", "udgt", *amount_udgt)?;
                }
                TxMessage::Data { from, .. }
                | TxMessage::DmsRegister { from, .. }
                | TxMessage::DmsPing { from }
                | TxMessage::DmsClaim { from, .. }
                | TxMessage::RewardBeginUnbond { from, .. }
                | TxMessage::RewardClaim { from }
                | TxMessage::ValidatorRotateKey { from, .. }
                | TxMessage::ValidatorExit { from, .. }
                | TxMessage::ValidatorWithdraw { from, .. } => {
                    if from != &tx.from {
                        return Err("Message sender differs from transaction sender".into());
                    }
                }
            }
        }
    } else {
        send(&tx.from, &tx.to, &tx.denom, tx.amount)?;
    }
    required.retain(|_, amount| *amount > 0);
    Ok(required)
}

#[cfg(test)]
mod unit_contract_tests {
    use super::*;

    #[test]
    fn reward_bonds_reserve_liquid_dgt_without_assuming_claims_or_unbond_credits() {
        let mut tx = Transaction::base("reward-reservation", "owner", "owner", 0, 100, 0)
            .with_messages(vec![
                TxMessage::RewardBond {
                    from: "owner".into(),
                    validator: "owner".into(),
                    amount_udgt: 9,
                },
                TxMessage::RewardBeginUnbond {
                    from: "owner".into(),
                    validator: "owner".into(),
                    amount_udgt: 9,
                },
                TxMessage::RewardClaim {
                    from: "owner".into(),
                },
                TxMessage::RewardBond {
                    from: "owner".into(),
                    validator: "validator".into(),
                    amount_udgt: 1,
                },
            ]);
        assert_eq!(
            required_balances(&tx).unwrap(),
            BTreeMap::from([("udgt".into(), 10), ("udrt".into(), 100)])
        );
        tx.messages.as_mut().unwrap()[2] = TxMessage::RewardClaim {
            from: "other".into(),
        };
        assert!(required_balances(&tx).is_err());
        tx.messages = Some(vec![
            TxMessage::RewardBond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: u128::MAX,
            },
            TxMessage::RewardBond {
                from: "owner".into(),
                validator: "validator".into(),
                amount_udgt: 1,
            },
        ]);
        assert!(required_balances(&tx).is_err());
    }

    #[test]
    fn native_aliases_convert_once_and_reject_overflow() {
        for (whole_denom, base_denom) in [("DGT", "udgt"), ("DRT", "udrt")] {
            let (denom, amount) = normalize_send(whole_denom, 1).unwrap();
            assert_eq!((&*denom, amount), (base_denom, 1_000_000));
            assert_eq!(normalize_send(&denom, amount).unwrap(), (denom, amount));
            assert_eq!(
                normalize_send(base_denom, u128::MAX).unwrap(),
                (base_denom.into(), u128::MAX)
            );
            let largest_whole = u128::MAX / 1_000_000;
            assert_eq!(
                normalize_send(whole_denom, largest_whole).unwrap().1,
                largest_whole * 1_000_000
            );
            assert!(normalize_send(whole_denom, largest_whole + 1).is_err());
        }
        assert!(normalize_send("legacy-dgt", 1).is_err());
    }
}
