//! Versioned transaction storage. Signature verification belongs to the node.
use crate::tx::Transaction;
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{Tx, TRANSACTION_FORMAT_VERSION};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8] = b"DYT-TX-RECORD\0";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SignedEnvelope {
    pub tx: Tx,
    pub public_key: String,
    pub signature: String,
    pub algorithm: String,
    pub version: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionRecord {
    pub version: u32,
    pub transaction: Transaction,
    pub signed_envelope: Option<SignedEnvelope>,
}
impl TransactionRecord {
    pub fn new(transaction: Transaction, signed_envelope: Option<SignedEnvelope>) -> Result<Self> {
        let record = Self {
            version: 1,
            transaction,
            signed_envelope,
        };
        record.validate(&record.transaction.hash)?;
        Ok(record)
    }
    pub fn validate(&self, key_hash: &str) -> Result<()> {
        ensure!(self.version == 1, "Unsupported transaction record version");
        let tx = &self.transaction;
        ensure!(
            !key_hash.is_empty() && tx.hash == key_hash,
            "Transaction record hash differs from key"
        );
        if let Some(signed) = &self.signed_envelope {
            ensure!(
                signed.version == TRANSACTION_FORMAT_VERSION,
                "Unsupported signed envelope version"
            );
            ensure!(!signed.algorithm.is_empty(), "Missing signed algorithm");
            ensure!(
                signed.tx.tx_hash()? == tx.hash,
                "Original transaction hash differs"
            );
            ensure!(
                signed.tx.nonce == tx.nonce
                    && signed.tx.fee == tx.fee
                    && signed.tx.chain_id == tx.chain_id
                    && signed.tx.memo == tx.memo,
                "Original transaction fields differ"
            );
            ensure!(
                tx.public_key.as_deref() == Some(signed.public_key.as_str())
                    && tx.signature.as_deref() == Some(signed.signature.as_str()),
                "Original authentication fields differ"
            );
            ensure!(
                signed
                    .tx
                    .msgs
                    .first()
                    .is_some_and(|m| m.sender() == tx.from),
                "Original sender differs"
            );
        }
        Ok(())
    }
    pub fn require_original_if_signed(&self) -> Result<()> {
        ensure!(
            (self.transaction.signature.is_none() && self.transaction.public_key.is_none())
                || self.signed_envelope.is_some(),
            "Signed transaction has no original envelope; migration required"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate(&self.transaction.hash)?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(serde_json::to_vec(self)?);
        Ok(bytes)
    }
    pub fn decode(key_hash: &str, bytes: &[u8]) -> Result<Self> {
        let json = bytes
            .strip_prefix(MAGIC)
            .context("Legacy or unknown transaction record; explicit migration required")?;
        let record: Self = serde_json::from_slice(json).context("Invalid transaction record")?;
        record.validate(key_hash)?;
        Ok(record)
    }
    pub fn matches(&self, tx: &Transaction) -> Result<bool> {
        Ok(serde_json::to_vec(&self.transaction)? == serde_json::to_vec(tx)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{state::Storage, tx::TxMessage};
    use std::collections::BTreeMap;
    fn tx() -> Transaction {
        Transaction::new("record", "alice", "bob", u128::MAX, 1, 0, None).with_messages(vec![
            TxMessage::Send {
                from: "alice".into(),
                to: "bob".into(),
                denom: "udgt".into(),
                amount: u128::MAX,
            },
            TxMessage::Data {
                from: "alice".into(),
                data: "data".into(),
            },
            TxMessage::DmsRegister {
                from: "alice".into(),
                beneficiary: "bob".into(),
                period: u128::MAX,
            },
            TxMessage::DmsPing {
                from: "alice".into(),
            },
            TxMessage::DmsClaim {
                from: "alice".into(),
                owner: "owner".into(),
            },
        ])
    }
    fn snapshot(storage: &Storage) -> BTreeMap<Vec<u8>, Vec<u8>> {
        storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|v| {
                let (k, v) = v.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    }
    #[test]
    fn all_tagged_messages_and_full_width_values_round_trip() {
        let transaction = tx();
        let record = TransactionRecord::new(transaction.clone(), None).unwrap();
        let bytes = record.encode().unwrap();
        let decoded = TransactionRecord::decode("record", &bytes).unwrap();
        assert!(decoded.matches(&transaction).unwrap());
        assert_eq!(decoded.encode().unwrap(), bytes);
        assert_eq!(decoded.transaction.messages.as_ref().unwrap().len(), 5);
        assert!(String::from_utf8(bytes)
            .unwrap()
            .contains(&u128::MAX.to_string()));
    }
    #[test]
    fn parsed_signed_envelope_retains_all_fields() {
        let original = Tx {
            chain_id: "test".into(),
            nonce: 0,
            msgs: vec![dytallix_protocol_types::Msg::Data {
                from: "alice".into(),
                data: "payload".into(),
            }],
            fee: 1,
            memo: "memo".into(),
        };
        let envelope = SignedEnvelope {
            tx: original.clone(),
            public_key: "fixture-public-key".into(),
            signature: "fixture-signature".into(),
            algorithm: "fixture-algorithm".into(),
            version: 1,
        };
        let mut transaction = Transaction::new(
            original.tx_hash().unwrap(),
            "alice",
            "alice",
            0,
            1,
            0,
            Some(envelope.signature.clone()),
        )
        .with_pqc(&envelope.public_key, "test", "memo");
        transaction.messages = Some(vec![TxMessage::Data {
            from: "alice".into(),
            data: "payload".into(),
        }]);
        let record = TransactionRecord::new(transaction, Some(envelope.clone())).unwrap();
        let decoded =
            TransactionRecord::decode(&record.transaction.hash, &record.encode().unwrap()).unwrap();
        assert_eq!(decoded.signed_envelope, Some(envelope));
        // This test checks storage, not cryptographic validity of fixture strings.
    }
    #[test]
    fn invalid_version_key_and_envelope_links_return_errors() {
        let mut record = TransactionRecord::new(tx(), None).unwrap();
        assert!(TransactionRecord::decode("other", &record.encode().unwrap()).is_err());
        record.version = 2;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(serde_json::to_vec(&record).unwrap());
        assert!(TransactionRecord::decode("record", &bytes).is_err());
        record.version = 1;
        record.signed_envelope = Some(SignedEnvelope {
            tx: Tx {
                chain_id: "test".into(),
                nonce: 0,
                msgs: vec![],
                fee: 1,
                memo: String::new(),
            },
            public_key: String::new(),
            signature: String::new(),
            algorithm: "fixture".into(),
            version: 1,
        });
        assert!(record.encode().is_err());
        assert!(TransactionRecord::decode("record", b"DYT-TX-RECORD\0{}").is_err());
    }
    #[test]
    fn pending_record_and_receipt_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let storage = Storage::open(path.clone()).unwrap();
        let transaction = tx();
        storage.put_pending_transaction(&transaction).unwrap();
        let before = snapshot(&storage);
        drop(storage);
        let storage = Storage::open(path).unwrap();
        assert!(storage
            .get_transaction_record("record")
            .unwrap()
            .unwrap()
            .matches(&transaction)
            .unwrap());
        assert_eq!(
            storage.get_receipt("record").unwrap().status,
            crate::receipts::TxStatus::Pending
        );
        assert_eq!(snapshot(&storage), before);
    }
    #[test]
    fn same_hash_cannot_replace_a_stored_body_or_partially_write_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        let transaction = tx();
        storage.put_pending_transaction(&transaction).unwrap();
        let before = snapshot(&storage);
        let mut changed = transaction.clone();
        changed.memo = "different".into();
        assert!(storage.put_pending_transaction(&changed).is_err());
        assert!(storage.put_tx(&changed).is_err());
        assert_eq!(snapshot(&storage), before);
        storage.put_pending_transaction(&transaction).unwrap();
        assert_eq!(snapshot(&storage), before);
    }
    #[test]
    fn legacy_binary_tagged_messages_require_explicit_migration() {
        let transaction = tx();
        let legacy = bincode::serialize(&transaction).unwrap();
        // Ordinary round-trip control shows why this binary format is unsuitable.
        assert!(bincode::deserialize::<Transaction>(&legacy).is_err());
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        storage.db.put("tx:record", &legacy).unwrap();
        let before = snapshot(&storage);
        assert!(storage
            .get_transaction_record("record")
            .unwrap_err()
            .to_string()
            .contains("migration"));
        assert!(storage.put_pending_transaction(&transaction).is_err());
        assert_eq!(snapshot(&storage), before);
    }
}
