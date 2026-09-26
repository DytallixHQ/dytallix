//! Development signed-input conversion and stored record verification.
//! Address ownership and mainnet gas policy require separate protocol decisions.
use crate::storage::{
    transaction_record::TransactionRecord,
    tx::{Transaction, TxMessage},
};
use crate::types::{Msg, SignedTx};
use anyhow::{ensure, Context, Result};

#[derive(Debug, thiserror::Error)]
pub enum NormalizationError {
    #[error("{0}")]
    Transaction(String),
    #[error("{0}")]
    Fee(String),
}

fn gas_limit_from_signed_fee(signed_fee: u128, gas_price: u64) -> Result<u64, String> {
    if gas_price == 0 {
        return Err("minimum gas price is zero".to_string());
    }

    let gas_price = u128::from(gas_price);
    if signed_fee < gas_price {
        return Err(format!(
            "fee {} is below the minimum billable unit {}",
            signed_fee, gas_price
        ));
    }
    if signed_fee % gas_price != 0 {
        return Err(format!(
            "fee {} must be an exact multiple of min gas price {}",
            signed_fee, gas_price
        ));
    }

    let gas_limit = signed_fee / gas_price;
    u64::try_from(gas_limit)
        .map_err(|_| format!("fee {} is too large to convert into gas units", signed_fee))
}

/// Convert a parsed signed input using the development admission gas price.
/// This function does not verify the signature or account ownership.
pub(crate) fn normalize(
    signed_tx: &SignedTx,
    gas_price: u64,
) -> Result<Transaction, NormalizationError> {
    normalize_with_policy(signed_tx, gas_price, false)
}
pub(crate) fn normalize_penalty(
    signed_tx: &SignedTx,
    gas_price: u64,
) -> Result<Transaction, NormalizationError> {
    normalize_with_policy(signed_tx, gas_price, true)
}
fn normalize_with_policy(
    signed_tx: &SignedTx,
    gas_price: u64,
    owner_withdrawals: bool,
) -> Result<Transaction, NormalizationError> {
    validate_origin_authorization(signed_tx, false, owner_withdrawals)
        .map_err(|error| NormalizationError::Transaction(error.to_string()))?;
    let from = signed_tx
        .first_from_address()
        .ok_or_else(|| NormalizationError::Transaction("no sender address found".into()))?;
    let mut legacy_tx = Transaction::new(
        signed_tx
            .tx_hash()
            .map_err(|e| NormalizationError::Transaction(e.to_string()))?,
        from.to_string(),
        from.to_string(),
        0, // populate below based on msgs
        signed_tx.tx.fee,
        signed_tx.tx.nonce,
        Some(signed_tx.signature.clone()),
    )
    .with_pqc(
        signed_tx.public_key.clone(),
        signed_tx.tx.chain_id.clone(),
        signed_tx.tx.memo.clone(),
    );

    let mut tx_messages = Vec::new();

    // Retain the existing display total; execution uses individual messages.
    let mut total_amount: u128 = 0;
    let mut first_to = legacy_tx.to.clone();
    let mut first_denom = "udgt".to_string(); // Default for backward compatibility
    for msg in &signed_tx.tx.msgs {
        match msg {
            Msg::Send {
                to,
                amount,
                denom,
                from: msg_from,
                ..
            } => {
                let (micro_denom, micro_amount) =
                    crate::transaction_cost::normalize_send(denom, *amount)
                        .map_err(NormalizationError::Transaction)?;
                total_amount = total_amount.checked_add(micro_amount).ok_or_else(|| {
                    NormalizationError::Transaction("Transaction display total exceeds u128".into())
                })?;

                tx_messages.push(TxMessage::Send {
                    from: msg_from.clone(),
                    to: to.clone(),
                    denom: micro_denom.clone(),
                    amount: micro_amount,
                });

                if first_to == from {
                    first_to = to.clone();
                    first_denom = micro_denom;
                }
            }
            Msg::Data {
                from: msg_from,
                data,
            } => {
                tx_messages.push(TxMessage::Data {
                    from: msg_from.clone(),
                    data: data.clone(),
                });
            }
            Msg::DmsRegister {
                from,
                beneficiary,
                period,
            } => {
                tx_messages.push(TxMessage::DmsRegister {
                    from: from.clone(),
                    beneficiary: beneficiary.clone(),
                    period: *period,
                });
            }
            Msg::RewardBond {
                from,
                validator,
                amount_udgt,
            } => {
                tx_messages.push(TxMessage::RewardBond {
                    from: from.clone(),
                    validator: validator.clone(),
                    amount_udgt: *amount_udgt,
                });
            }
            Msg::RewardBeginUnbond {
                from,
                validator,
                amount_udgt,
            } => {
                tx_messages.push(TxMessage::RewardBeginUnbond {
                    from: from.clone(),
                    validator: validator.clone(),
                    amount_udgt: *amount_udgt,
                });
            }
            Msg::RewardClaim { from } => {
                tx_messages.push(TxMessage::RewardClaim { from: from.clone() });
            }
            Msg::ValidatorRegister {
                from,
                validator,
                consensus_pubkey,
                proof,
                expires_at_height,
                amount_udgt,
            } => {
                tx_messages.push(TxMessage::ValidatorRegister {
                    from: from.clone(),
                    validator: validator.clone(),
                    consensus_pubkey: consensus_pubkey.clone(),
                    proof: proof.clone(),
                    expires_at_height: *expires_at_height,
                    amount_udgt: *amount_udgt,
                });
            }
            Msg::ValidatorRotateKey {
                from,
                validator,
                consensus_pubkey,
                proof,
                expires_at_height,
            } => {
                tx_messages.push(TxMessage::ValidatorRotateKey {
                    from: from.clone(),
                    validator: validator.clone(),
                    consensus_pubkey: consensus_pubkey.clone(),
                    proof: proof.clone(),
                    expires_at_height: *expires_at_height,
                });
            }
            Msg::ValidatorExit { from, validator } => {
                tx_messages.push(TxMessage::ValidatorExit {
                    from: from.clone(),
                    validator: validator.clone(),
                });
            }
            Msg::ValidatorWithdraw { from, unbond_id } => {
                tx_messages.push(TxMessage::ValidatorWithdraw {
                    from: from.clone(),
                    unbond_id: unbond_id.clone(),
                });
            }
            Msg::DmsPing { from } => {
                tx_messages.push(TxMessage::DmsPing { from: from.clone() });
            }
            Msg::DmsClaim { from, owner } => {
                tx_messages.push(TxMessage::DmsClaim {
                    from: from.clone(),
                    owner: owner.clone(),
                });
            }
        }
    }
    legacy_tx.amount = total_amount;
    legacy_tx.to = first_to;
    legacy_tx.denom = first_denom;
    legacy_tx = legacy_tx.with_messages(tx_messages);

    legacy_tx.gas_price = gas_price;
    legacy_tx.gas_limit =
        gas_limit_from_signed_fee(signed_tx.tx.fee, gas_price).map_err(NormalizationError::Fee)?;
    Ok(legacy_tx)
}

/// Reverify an original signed input and its complete execution representation.
/// Gas price is stored development policy input; the signature covers total fee.
/// Unsigned internal development records retain their existing execution rules.
pub(crate) fn verify_record(
    record: &TransactionRecord,
    expected_chain_id: Option<&str>,
) -> Result<()> {
    verify_record_with_policy(record, expected_chain_id, false)
}
fn verify_record_with_policy(
    record: &TransactionRecord,
    expected_chain_id: Option<&str>,
    owner_withdrawals: bool,
) -> Result<()> {
    record.validate(&record.transaction.hash)?;
    record.require_original_if_signed()?;
    let Some(original) = &record.signed_envelope else {
        ensure!(
            !contains_reward_messages(&record.transaction),
            "Reward messages require an original signed envelope"
        );
        return Ok(());
    };
    let chain_id = expected_chain_id
        .filter(|id| !id.is_empty())
        .context("Missing chain ID for signed transaction verification")?;
    let signed = SignedTx {
        tx: original.tx.clone(),
        public_key: original.public_key.clone(),
        signature: original.signature.clone(),
        algorithm: original.algorithm.clone(),
        version: original.version,
    };
    signed
        .tx
        .validate(chain_id)
        .context("Invalid stored signed transaction")?;
    signed
        .verify()
        .context("Stored original signature verification failed")?;
    let normalized =
        normalize_with_policy(&signed, record.transaction.gas_price, owner_withdrawals)?;
    ensure!(
        record.matches(&normalized)?,
        "Stored transaction differs from original signed input conversion"
    );
    Ok(())
}

/// Every transaction in reward mode must prove immutable origin ownership.
/// Legacy internal unsigned records remain limited to databases without reward mode.
pub fn verify_reward_mode_record(record: &TransactionRecord, chain_id: Option<&str>) -> Result<()> {
    verify_mode_record(record, chain_id, false)
}
pub(crate) fn verify_penalty_mode_record(
    record: &TransactionRecord,
    chain_id: Option<&str>,
) -> Result<()> {
    verify_mode_record(record, chain_id, true)
}
fn verify_mode_record(
    record: &TransactionRecord,
    chain_id: Option<&str>,
    owner_withdrawals: bool,
) -> Result<()> {
    let original = record
        .signed_envelope
        .as_ref()
        .context("Reward mode requires an original signed envelope")?;
    verify_record_with_policy(record, chain_id, owner_withdrawals)?;
    validate_origin_authorization(
        &SignedTx {
            tx: original.tx.clone(),
            public_key: original.public_key.clone(),
            signature: original.signature.clone(),
            algorithm: original.algorithm.clone(),
            version: original.version,
        },
        true,
        owner_withdrawals,
    )
}

/// The selected reward mode supports immutable origin-key authorization only.
/// A future key-rotation policy requires explicit account authorization state.
fn validate_origin_authorization(
    signed: &SignedTx,
    require_all: bool,
    owner_withdrawals: bool,
) -> Result<()> {
    use crate::addr::{AccountAddress, AddressNetwork, OriginKeyAlgorithm};
    use crate::crypto::PQCAlgorithm;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use std::str::FromStr;
    if !require_all
        && !signed.tx.msgs.iter().any(|message| {
            matches!(
                message,
                Msg::RewardBond { .. }
                    | Msg::RewardBeginUnbond { .. }
                    | Msg::RewardClaim { .. }
                    | Msg::ValidatorRegister { .. }
                    | Msg::ValidatorRotateKey { .. }
                    | Msg::ValidatorExit { .. }
                    | Msg::ValidatorWithdraw { .. }
            )
        })
    {
        return Ok(());
    }
    let from = signed
        .first_from_address()
        .context("Reward transaction has no sender")?;
    for message in &signed.tx.msgs {
        message.validate()?;
        ensure!(
            message.sender() == from,
            "Message sender differs from transaction sender"
        );
    }
    let algorithm = match PQCAlgorithm::from_str(&signed.algorithm)? {
        PQCAlgorithm::MlDsa65 => OriginKeyAlgorithm::MlDsa65,
        PQCAlgorithm::MlDsa87 => OriginKeyAlgorithm::MlDsa87,
        PQCAlgorithm::Dilithium5 => OriginKeyAlgorithm::LegacyDilithium5,
        _ => anyhow::bail!("Reward signer algorithm has no origin address mapping"),
    };
    if signed.tx.msgs.iter().any(|message| {
        matches!(
            message,
            Msg::ValidatorRegister { .. }
                | Msg::ValidatorRotateKey { .. }
                | Msg::ValidatorExit { .. }
        ) || (!owner_withdrawals && matches!(message, Msg::ValidatorWithdraw { .. }))
    }) {
        ensure!(
            algorithm == OriginKeyAlgorithm::MlDsa65,
            "Validator operator requires an ML-DSA-65 account"
        );
    }
    let public_key = B64
        .decode(&signed.public_key)
        .context("Invalid reward public key encoding")?;
    let network = [
        AddressNetwork::Mainnet,
        AddressNetwork::Testnet,
        AddressNetwork::Development,
    ]
    .into_iter()
    .find(|network| AccountAddress::decode(*network, from).is_ok())
    .context("Reward sender is not a canonical account address")?;
    let expected =
        AccountAddress::from_origin_key(network, &signed.tx.chain_id, algorithm, &public_key)?
            .encode();
    ensure!(
        from == expected,
        "Reward sender differs from signing origin key"
    );
    Ok(())
}

pub(crate) fn contains_reward_messages(tx: &Transaction) -> bool {
    tx.messages.as_ref().is_some_and(|messages| {
        messages.iter().any(|message| {
            matches!(
                message,
                TxMessage::RewardBond { .. }
                    | TxMessage::RewardBeginUnbond { .. }
                    | TxMessage::RewardClaim { .. }
                    | TxMessage::ValidatorRegister { .. }
                    | TxMessage::ValidatorRotateKey { .. }
                    | TxMessage::ValidatorExit { .. }
                    | TxMessage::ValidatorWithdraw { .. }
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{ActivePQC, PQC};
    use crate::storage::transaction_record::SignedEnvelope;
    use crate::types::tx::Tx;

    fn signed(msgs: impl FnOnce(&str) -> Vec<Msg>, fee: u128) -> SignedTx {
        let (sk, pk) = ActivePQC::keypair();
        let algorithm = crate::addr::OriginKeyAlgorithm::MlDsa65;
        let sender = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            "signed-input-local",
            algorithm,
            &pk,
        )
        .unwrap();
        SignedTx::sign(
            Tx {
                chain_id: "signed-input-local".into(),
                nonce: 7,
                msgs: msgs(&sender),
                fee,
                memo: "original memo".into(),
            },
            &sk,
            &pk,
        )
        .unwrap()
    }
    fn send(from: &str, denom: &str, amount: u128) -> Msg {
        Msg::Send {
            from: from.into(),
            to: "receiver".into(),
            denom: denom.into(),
            amount,
        }
    }
    fn record(signed: &SignedTx, price: u64) -> TransactionRecord {
        let original: SignedEnvelope =
            serde_json::from_value(serde_json::to_value(signed).unwrap()).unwrap();
        TransactionRecord::new(normalize(signed, price).unwrap(), Some(original)).unwrap()
    }

    #[test]
    fn signed_aliases_preserve_original_and_reconstruct_base_units() {
        for (denom, expected, amount) in [
            ("DGT", "udgt", 2_000_000),
            ("dGt", "udgt", 2_000_000),
            ("DRT", "udrt", 2_000_000),
            ("dRt", "udrt", 2_000_000),
            ("udgt", "udgt", 2),
            ("UDGT", "udgt", 2),
            ("udrt", "udrt", 2),
            ("UDRT", "udrt", 2),
        ] {
            let signed = signed(|from| vec![send(from, denom, 2)], 21_000_000);
            let record = record(&signed, 2_000);
            assert_eq!(record.transaction.amount, amount);
            assert_eq!(record.transaction.denom, expected);
            assert_eq!(record.transaction.gas_limit, 10_500);
            assert_eq!(record.signed_envelope.as_ref().unwrap().tx, signed.tx);
            verify_record(&record, Some("signed-input-local")).unwrap();
        }
    }
    #[test]
    fn ordered_messages_and_self_send_display_remain_compatible() {
        let signed = signed(
            |from| {
                vec![
                    Msg::Send {
                        from: from.into(),
                        to: from.into(),
                        denom: "DRT".into(),
                        amount: 2,
                    },
                    Msg::Data {
                        from: from.into(),
                        data: "data".into(),
                    },
                    send(from, "DGT", 3),
                    Msg::DmsRegister {
                        from: from.into(),
                        beneficiary: "beneficiary".into(),
                        period: 9,
                    },
                    Msg::DmsPing { from: from.into() },
                    Msg::DmsClaim {
                        from: from.into(),
                        owner: "owner".into(),
                    },
                ]
            },
            21_000_000,
        );
        let record = record(&signed, 1_000);
        let tx = &record.transaction;
        assert_eq!(tx.to, "receiver");
        assert_eq!(tx.denom, "udgt");
        assert_eq!(tx.amount, 5_000_000);
        let messages = serde_json::to_value(tx.messages.as_ref().unwrap()).unwrap();
        assert_eq!(messages[0]["denom"], "udrt");
        assert_eq!(messages[0]["amount"], "2000000");
        assert_eq!(messages[1]["data"], "data");
        assert_eq!(messages[2]["amount"], "3000000");
        assert_eq!(messages[3]["beneficiary"], "beneficiary");
        assert_eq!(messages[3]["period"], "9");
        assert_eq!(messages[4]["type"], "dmsping");
        assert_eq!(messages[5]["owner"], "owner");
        verify_record(&record, Some("signed-input-local")).unwrap();
    }
    #[test]
    fn full_width_amount_and_data_only_round_trip() {
        for signed in [
            signed(|from| vec![send(from, "udgt", u128::MAX)], 21_000_000),
            signed(
                |from| {
                    vec![Msg::Data {
                        from: from.into(),
                        data: "payload".into(),
                    }]
                },
                21_000_000,
            ),
        ] {
            let record = record(&signed, 1_000);
            let bytes = record.encode().unwrap();
            let decoded = TransactionRecord::decode(&record.transaction.hash, &bytes).unwrap();
            verify_record(&decoded, Some("signed-input-local")).unwrap();
            assert_eq!(decoded.encode().unwrap(), bytes);
            if matches!(signed.tx.msgs[0], Msg::Data { .. }) {
                assert_eq!(decoded.transaction.to, signed.first_from_address().unwrap());
                assert_eq!(decoded.transaction.amount, 0);
                assert_eq!(decoded.transaction.denom, "udgt");
            } else {
                assert_eq!(decoded.transaction.amount, u128::MAX);
            }
        }
    }
    #[test]
    fn conversion_rejects_unrepresentable_amounts_and_gas() {
        for (fee, price) in [(1, 0), (1, 2), (3, 2), (u128::MAX, 1)] {
            assert!(gas_limit_from_signed_fee(fee, price).is_err());
        }
        assert_eq!(
            gas_limit_from_signed_fee(u128::from(u64::MAX), 1).unwrap(),
            u64::MAX
        );
        let overflow = signed(|from| vec![send(from, "DGT", u128::MAX)], 1);
        assert!(matches!(
            normalize(&overflow, 1),
            Err(NormalizationError::Transaction(_))
        ));
        let total = signed(
            |from| vec![send(from, "udgt", u128::MAX), send(from, "udrt", 1)],
            1,
        );
        assert!(matches!(
            normalize(&total, 1),
            Err(NormalizationError::Transaction(_))
        ));
    }
    #[test]
    fn signed_record_requires_chain_and_original_signature() {
        let signed = signed(|from| vec![send(from, "udgt", 2)], 21_000_000);
        let record = record(&signed, 1_000);
        verify_record(&record, Some("signed-input-local")).unwrap();
        for chain in [None, Some(""), Some("other-chain")] {
            assert!(verify_record(&record, chain).is_err());
        }
        let mut missing = record.clone();
        missing.signed_envelope = None;
        assert!(verify_record(&missing, Some("signed-input-local")).is_err());
        let mut invalid = record;
        invalid.signed_envelope.as_mut().unwrap().signature = "AA==".into();
        invalid.transaction.signature = Some("AA==".into());
        assert!(verify_record(&invalid, Some("signed-input-local"))
            .unwrap_err()
            .to_string()
            .contains("signature verification failed"));
    }
    #[test]
    fn record_verification_checks_complete_execution_representation() {
        let signed = signed(|from| vec![send(from, "DGT", 2)], 21_000_000);
        let original = record(&signed, 1_000);
        let mut variants = vec![original.clone(); 6];
        variants[0].transaction.amount += 1;
        variants[1].transaction.to = "different".into();
        variants[2].transaction.denom = "udrt".into();
        variants[3].transaction.gas_limit += 1;
        variants[4].transaction.messages = None;
        variants[5].transaction.messages = Some(vec![TxMessage::Data {
            from: signed.first_from_address().unwrap().into(),
            data: "different".into(),
        }]);
        for record in variants {
            assert!(verify_record(&record, Some("signed-input-local"))
                .unwrap_err()
                .to_string()
                .contains("differs from original signed input conversion"));
        }
    }
    #[test]
    fn reward_messages_preserve_signed_amounts_and_bind_every_sender() {
        let signed = signed(
            |from| {
                vec![
                    Msg::RewardBond {
                        from: from.into(),
                        validator: "validator".into(),
                        amount_udgt: u128::MAX,
                    },
                    Msg::RewardBeginUnbond {
                        from: from.into(),
                        validator: "validator".into(),
                        amount_udgt: 1,
                    },
                    Msg::RewardClaim { from: from.into() },
                ]
            },
            21_000_000,
        );
        let original = record(&signed, 1_000);
        verify_record(&original, Some("signed-input-local")).unwrap();
        let messages =
            serde_json::to_value(original.transaction.messages.as_ref().unwrap()).unwrap();
        assert_eq!(messages[0]["type"], "reward_bond");
        assert_eq!(messages[0]["amount_udgt"], u128::MAX.to_string());
        assert_eq!(messages[1]["amount_udgt"], "1");
        assert_eq!(messages[2]["type"], "reward_claim");
        let mut mismatched = signed.clone();
        mismatched.tx.msgs[2] = Msg::RewardClaim {
            from: "another-owner".into(),
        };
        assert!(normalize(&mismatched, 1_000)
            .unwrap_err()
            .to_string()
            .contains("Message sender differs"));
        let mut changed = original;
        changed.transaction.messages.as_mut().unwrap()[0] = TxMessage::RewardBond {
            from: signed.first_from_address().unwrap().into(),
            validator: "other-validator".into(),
            amount_udgt: u128::MAX,
        };
        assert!(verify_record(&changed, Some("signed-input-local")).is_err());
    }

    #[test]
    fn reward_envelope_rejects_valid_signature_for_another_owner() {
        let victim = signed(
            |from| vec![Msg::RewardClaim { from: from.into() }],
            21_000_000,
        );
        let victim_address = victim.first_from_address().unwrap().to_owned();
        let unauthorized = signed(
            |_| {
                vec![Msg::RewardClaim {
                    from: victim_address,
                }]
            },
            21_000_000,
        );
        unauthorized.verify().unwrap();
        assert!(normalize(&unauthorized, 1_000)
            .unwrap_err()
            .to_string()
            .contains("Reward sender differs from signing origin key"));
    }

    #[test]
    fn reward_mode_binds_ordinary_message_senders_to_the_signing_key() {
        let authorized = signed(|from| vec![send(from, "udgt", 1)], 21_000_000);
        verify_reward_mode_record(&record(&authorized, 1_000), Some("signed-input-local")).unwrap();
        let victim = authorized.first_from_address().unwrap().to_owned();
        let unauthorized = signed(|_| vec![send(&victim, "udgt", 1)], 21_000_000);
        let unauthorized_record = record(&unauthorized, 1_000);
        verify_record(&unauthorized_record, Some("signed-input-local")).unwrap();
        assert!(
            verify_reward_mode_record(&unauthorized_record, Some("signed-input-local"))
                .unwrap_err()
                .to_string()
                .contains("signing origin key")
        );
        let unsigned = TransactionRecord::new(
            Transaction::base("unsigned-mode", "owner", "receiver", 1, 21_000, 0),
            None,
        )
        .unwrap();
        assert!(verify_reward_mode_record(&unsigned, Some("signed-input-local")).is_err());
    }

    #[test]
    fn reward_records_require_original_signed_envelope() {
        let tx = Transaction::base("unsigned-reward", "owner", "owner", 0, 21_000, 0)
            .with_messages(vec![TxMessage::RewardClaim {
                from: "owner".into(),
            }]);
        let record = TransactionRecord::new(tx, None).unwrap();
        assert!(verify_record(&record, Some("signed-input-local"))
            .unwrap_err()
            .to_string()
            .contains("original signed envelope"));
    }

    #[test]
    fn unsigned_internal_development_record_keeps_legacy_gas_rules() {
        let tx = Transaction::new("internal", "alice", "bob", 1, 21_000, 0, None);
        let record = TransactionRecord::new(tx, None).unwrap();
        verify_record(&record, None).unwrap();
    }
}
