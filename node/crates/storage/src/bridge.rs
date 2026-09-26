use rocksdb::DB;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// Persistent bridge store
// Keys:
// bridge:halted -> 0|1
// bridge:validators -> JSON array of Validator { id, pubkey }
// bridge:custody:{asset} -> u128 (bincode)
// bridge:pending:{id} -> JSON BridgeMessage
// bridge:applied:{id} -> JSON BridgeMessage

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeValidator {
    pub id: String,
    pub pubkey: String, // base64 ed25519
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: String, // unique message id (hash of content from source chain)
    pub source_chain: String,
    pub dest_chain: String,
    pub asset: String,
    pub amount: u128,
    pub recipient: String,
    pub signatures: Vec<String>, // provided signatures (base64)
    pub signers: Vec<String>, // corresponding validator ids for provided signatures (same length)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStateDebug {
    pub halted: bool,
    pub validators: Vec<BridgeValidator>,
    pub custody: HashMap<String, u128>,
    pub pending: Vec<String>,
    pub applied: Vec<String>,
}

pub struct BridgeStore<'a> {
    pub db: &'a DB,
}
impl<'a> BridgeStore<'a> {
    fn key_halted() -> &'static str {
        "bridge:halted"
    }
    fn key_validators() -> &'static str {
        "bridge:validators"
    }
    fn key_custody(asset: &str) -> String {
        format!("bridge:custody:{asset}")
    }
    fn key_pending(id: &str) -> String {
        format!("bridge:pending:{id}")
    }
    fn key_applied(id: &str) -> String {
        format!("bridge:applied:{id}")
    }

    pub fn is_halted(&self) -> bool {
        self.db
            .get(Self::key_halted())
            .ok()
            .flatten()
            .map(|v| v == b"1")
            .unwrap_or(false)
    }
    pub fn set_halted(&self, halted: bool) -> anyhow::Result<()> {
        self.db
            .put(Self::key_halted(), if halted { b"1" } else { b"0" })?;
        Ok(())
    }

    pub fn get_validators(&self) -> Vec<BridgeValidator> {
        self.db
            .get(Self::key_validators())
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    }
    pub fn set_validators(&self, vals: &[BridgeValidator]) -> anyhow::Result<()> {
        self.db
            .put(Self::key_validators(), serde_json::to_vec(vals)?)?;
        Ok(())
    }

    pub fn get_custody(&self, asset: &str) -> u128 {
        self.db
            .get(Self::key_custody(asset))
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize(&v).ok())
            .unwrap_or(0)
    }
    pub fn add_custody(&self, asset: &str, amount: u128) -> anyhow::Result<u128> {
        let cur = self.get_custody(asset);
        let new = cur.saturating_add(amount);
        self.db
            .put(Self::key_custody(asset), bincode::serialize(&new)?)?;
        Ok(new)
    }

    pub fn put_pending(&self, msg: &BridgeMessage) -> anyhow::Result<()> {
        self.db
            .put(Self::key_pending(&msg.id), serde_json::to_vec(msg)?)?;
        Ok(())
    }
    pub fn mark_applied(&self, id: &str) -> anyhow::Result<()> {
        if let Some(raw) = self.db.get(Self::key_pending(id)).ok().flatten() {
            self.db.delete(Self::key_pending(id))?;
            self.db.put(Self::key_applied(id), raw)?;
        }
        Ok(())
    }
    pub fn list_pending(&self) -> Vec<String> {
        let mut out = vec![];
        let iter = self.db.prefix_iterator(b"bridge:pending:");
        for kv in iter.flatten() {
            if let Ok(key) = std::str::from_utf8(&kv.0) {
                if let Some(id) = key.rsplit(':').next() {
                    // use rsplit + next (O(1))
                    out.push(id.to_string());
                }
            }
        }
        out
    }
    pub fn list_applied(&self) -> Vec<String> {
        let mut out = vec![];
        let iter = self.db.prefix_iterator(b"bridge:applied:");
        for kv in iter.flatten() {
            if let Ok(key) = std::str::from_utf8(&kv.0) {
                if let Some(id) = key.rsplit(':').next() {
                    out.push(id.to_string());
                }
            }
        }
        out
    }

    pub fn build_debug_state(&self) -> BridgeStateDebug {
        let mut custody = HashMap::new();
        // naive scan (small scale) - in production would use column families or structured iteration.
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for kv in iter.flatten() {
            if let Ok(key) = std::str::from_utf8(&kv.0) {
                if let Some(rest) = key.strip_prefix("bridge:custody:") {
                    // avoid starts_with+split
                    if let Ok(v) = bincode::deserialize::<u128>(&kv.1) {
                        custody.insert(rest.to_string(), v);
                    }
                }
            }
        }
        BridgeStateDebug {
            halted: self.is_halted(),
            validators: self.get_validators(),
            custody,
            pending: self.list_pending(),
            applied: self.list_applied(),
        }
    }

    pub fn has_message(&self, id: &str) -> bool {
        self.db.get(Self::key_pending(id)).ok().flatten().is_some()
            || self.db.get(Self::key_applied(id)).ok().flatten().is_some()
    }
}

// Signature verification logic (ed25519) requiring >=2/3 validators
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{PublicKey, Signature, Verifier};

pub fn verify_bridge_message(
    msg: &BridgeMessage,
    validators: &[BridgeValidator],
) -> Result<(), String> {
    if msg.signatures.len() != msg.signers.len() {
        return Err("SignersSignaturesLengthMismatch".into());
    }
    // Map validators
    let map: HashMap<String, String> = validators
        .iter()
        .map(|v| (v.id.clone(), v.pubkey.clone()))
        .collect();
    let mut unique_signers = HashSet::new();
    let mut valid_count = 0u32;
    let payload = format!(
        "{}:{}:{}:{}:{}:{}",
        msg.id, msg.source_chain, msg.dest_chain, msg.asset, msg.amount, msg.recipient
    );
    for (i, signer) in msg.signers.iter().enumerate() {
        if unique_signers.contains(signer) {
            continue;
        }
        let pk_b64 = match map.get(signer) {
            Some(p) => p,
            None => continue,
        };
        let sig_b64 = &msg.signatures[i];
        let (Ok(pk_bytes), Ok(sig_bytes)) = (B64.decode(pk_b64), B64.decode(sig_b64)) else {
            continue;
        };
        let (Ok(pk), Ok(sig)) = (
            PublicKey::from_bytes(&pk_bytes),
            Signature::from_bytes(&sig_bytes),
        ) else {
            continue;
        };
        if pk.verify(payload.as_bytes(), &sig).is_ok() {
            unique_signers.insert(signer.clone());
            valid_count += 1;
        }
    }
    let total = validators.len() as f32;
    let needed = ((total * 2.0) / 3.0).ceil() as u32; // >= 2/3
    if valid_count >= needed {
        Ok(())
    } else {
        Err(format!(
            "InsufficientQuorum valid={valid_count} needed={needed}"
        ))
    }
}
