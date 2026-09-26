// Host environment for WASM contracts
// This module provides scaffolding for future host functions (storage, crypto)
// For now methods operate only on internal static state for the demo contract.

use crate::crypto::PQCManager;
use anyhow::Result;
use blake3;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::Linker; // Removed unused imports: Caller, Engine

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct HostExecutionContext {
    pub block_height: u64,
    pub block_time: i64,
    pub caller: String,
    pub deployer: String,
    pub input: Vec<u8>,
}

#[derive(Clone)]
pub struct HostEnv {
    inner: Arc<HostEnvInner>,
}

#[derive(Debug)]
struct HostEnvInner {
    ctx: Mutex<HostExecutionContext>,
    kv: Mutex<HashMap<Vec<u8>, Vec<u8>>>, // simple deterministic in-memory storage
    logs: Mutex<Vec<String>>,             // structured logs captured per execution
    output: Mutex<Vec<u8>>,               // captured output from contract execution
    gas_table: GasTable,
    pqc: Arc<PQCManager>,
}

impl std::fmt::Debug for HostEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostEnv").finish()
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct GasTable {
    pub storage_get: u64,
    pub storage_set: u64,
    pub storage_delete: u64,
    pub crypto_hash: u64,
    pub crypto_verify: u64,
    pub env_read: u64,
    pub env_write: u64,
    pub log: u64,
}

impl Default for GasTable {
    fn default() -> Self {
        Self {
            storage_get: 40,
            storage_set: 80,
            storage_delete: 50,
            crypto_hash: 15, // per 32-byte chunk charged additionally inside host fn
            crypto_verify: 5000, // PQ signature verification baseline
            env_read: 5,
            env_write: 5,
            log: 30,
        }
    }
}

impl Default for HostEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl HostEnv {
    pub fn new() -> Self {
        panic!("HostEnv::new() without PQCManager removed. Use HostEnv::with_pqc(pqc_manager)");
    }
    pub fn with_pqc(pqc: Arc<PQCManager>) -> Self {
        Self {
            inner: Arc::new(HostEnvInner {
                ctx: Mutex::new(HostExecutionContext::default()),
                kv: Mutex::new(HashMap::new()),
                logs: Mutex::new(Vec::new()),
                output: Mutex::new(Vec::new()),
                gas_table: GasTable::default(),
                pqc,
            }),
        }
    }

    #[allow(dead_code)]
    pub fn set_context(&self, ctx: HostExecutionContext) {
        if let Ok(mut guard) = self.inner.ctx.lock() {
            *guard = ctx;
        }
    }

    #[allow(dead_code)]
    pub fn context(&self) -> HostExecutionContext {
        self.inner
            .ctx
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| HostExecutionContext::default())
    }

    #[allow(dead_code)]
    pub fn gas_table(&self) -> GasTable {
        self.inner.gas_table
    }

    // Internal helpers used by host functions
    pub fn storage_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.kv.lock().unwrap().get(key).cloned()
    }
    pub fn storage_set(&self, key: Vec<u8>, value: Vec<u8>) {
        self.inner.kv.lock().unwrap().insert(key, value);
    }
    pub fn storage_delete(&self, key: &[u8]) -> bool {
        self.inner.kv.lock().unwrap().remove(key).is_some()
    }
    pub fn push_log(&self, msg: String) {
        eprintln!("HostEnv::push_log: {}", msg);
        self.inner.logs.lock().unwrap().push(msg);
    }
    #[allow(dead_code)]
    pub fn take_logs(&self) -> Vec<String> {
        self.inner.logs.lock().unwrap().drain(..).collect()
    }

    pub fn write_output(&self, data: &[u8]) {
        let mut out = self.inner.output.lock().unwrap();
        *out = data.to_vec();
    }

    pub fn take_output(&self) -> Vec<u8> {
        let mut out = self.inner.output.lock().unwrap();
        let res = out.clone();
        out.clear();
        res
    }

    pub fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }

    pub fn pqc_verify(&self, msg: &[u8], sig: &[u8], algo: &str, pubkey: &[u8]) -> bool {
        // Wrap raw pieces into PQCSignature struct for existing manager verify
        let signature = crate::crypto::PQCSignature {
            signature: sig.to_vec(),
            algorithm: algo.to_string(),
            nonce: 0,
            timestamp: 0,
        };
        self.inner
            .pqc
            .verify_signature(msg, &signature, pubkey)
            .unwrap_or(false)
    }

    pub fn register_host_functions<T>(&self, _linker: &mut Linker<T>) -> Result<()> {
        // Placeholder for future host function registration
        // Examples of future functions:
        // - storage_get/storage_set for persistent key-value storage
        // - crypto_hash for cryptographic operations
        // - emit_event for contract events
        // - block_info for accessing blockchain state

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::PQCManager;

    #[test]
    fn storage_roundtrip_is_deterministic() {
        let env = HostEnv::with_pqc(Arc::new(PQCManager::new().unwrap()));
        let k = b"key1".to_vec();
        let v = b"value1".to_vec();
        assert!(env.storage_get(&k).is_none());
        env.storage_set(k.clone(), v.clone());
        assert_eq!(env.storage_get(&k), Some(v.clone()));
        assert!(env.storage_delete(&k));
        assert!(env.storage_get(&k).is_none());
    }

    #[test]
    fn blake3_hash_matches_reference() {
        let env = HostEnv::with_pqc(Arc::new(PQCManager::new().unwrap()));
        let data = b"abc";
        let h1 = env.blake3_hash(data);
        let h2 = *blake3::hash(data).as_bytes();
        assert_eq!(h1, h2);
    }
}
