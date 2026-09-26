//! Storage Optimization Module for Cosmos WASM Contracts
//!
//! This module provides optimized storage access patterns, key compression,
//! and efficient data serialization to reduce storage costs and improve performance.

use cosmwasm_std::{from_json, to_json_vec, Addr, StdError, StdResult, Storage};
use cw_storage_plus::{Map, PrimaryKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;

/// Storage access pattern metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageMetrics {
    pub reads: u64,
    pub writes: u64,
    pub deletes: u64,
    pub key_size_total: u64,
    pub value_size_total: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

/// Optimized storage wrapper with caching and compression
pub struct OptimizedStorage {
    cache: HashMap<Vec<u8>, Vec<u8>>,
    cache_size_limit: usize,
    metrics: StorageMetrics,
    compression_enabled: bool,
    key_prefix_compression: HashMap<String, u8>,
}

impl OptimizedStorage {
    /// Create new optimized storage with default settings
    pub fn new() -> Self {
        Self::with_config(10000, true)
    }
}

impl Default for OptimizedStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizedStorage {
    /// Create optimized storage with custom configuration
    pub fn with_config(cache_size_limit: usize, compression_enabled: bool) -> Self {
        let mut key_prefix_compression = HashMap::new();

        // Common prefix mappings for key compression
        key_prefix_compression.insert("bridge_transactions".to_string(), 0x01);
        key_prefix_compression.insert("supported_tokens".to_string(), 0x02);
        key_prefix_compression.insert("validator_confirmations".to_string(), 0x03);
        key_prefix_compression.insert("contract_state".to_string(), 0x04);
        key_prefix_compression.insert("bridge_stats".to_string(), 0x05);
        key_prefix_compression.insert("ai_risk_scores".to_string(), 0x06);
        key_prefix_compression.insert("token_balances".to_string(), 0x07);
        key_prefix_compression.insert("validator_list".to_string(), 0x08);

        Self {
            cache: HashMap::new(),
            cache_size_limit,
            metrics: StorageMetrics::default(),
            compression_enabled,
            key_prefix_compression,
        }
    }

    /// Bind cached reads to exclusive access to one storage instance.
    pub fn session<'a>(&'a mut self, storage: &'a mut dyn Storage) -> StorageSession<'a> {
        self.clear_cache();
        StorageSession {
            optimizer: self,
            storage,
        }
    }

    /// Unbound compatibility reads always consult storage. Use a session for caching.
    pub fn optimized_read(&mut self, storage: &dyn Storage, key: &[u8]) -> Option<Vec<u8>> {
        self.clear_cache();
        self.read_bound(storage, key)
    }

    fn read_bound(&mut self, storage: &dyn Storage, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(value) = self.cache.get(key) {
            self.metrics.cache_hits = self.metrics.cache_hits.saturating_add(1);
            return Some(value.clone());
        }
        self.metrics.reads = self.metrics.reads.saturating_add(1);
        self.metrics.cache_misses = self.metrics.cache_misses.saturating_add(1);
        self.metrics.key_size_total = self.metrics.key_size_total.saturating_add(key.len() as u64);
        let value = storage.get(key)?;
        self.metrics.value_size_total = self
            .metrics
            .value_size_total
            .saturating_add(value.len() as u64);
        self.cache_value(key, &value);
        Some(value)
    }
    fn cache_value(&mut self, key: &[u8], value: &[u8]) {
        if self.cache.contains_key(key) || self.cache.len() < self.cache_size_limit {
            self.cache.insert(key.to_vec(), value.to_vec());
        }
    }
    pub fn optimized_write(&mut self, storage: &mut dyn Storage, key: &[u8], value: &[u8]) {
        storage.set(key, value);
        self.metrics.writes = self.metrics.writes.saturating_add(1);
        self.metrics.key_size_total = self.metrics.key_size_total.saturating_add(key.len() as u64);
        self.metrics.value_size_total = self
            .metrics
            .value_size_total
            .saturating_add(value.len() as u64);
        self.cache_value(key, value);
    }

    /// Optimized batch write for multiple operations
    pub fn batch_write(&mut self, storage: &mut dyn Storage, operations: Vec<(Vec<u8>, Vec<u8>)>) {
        for (key, value) in operations {
            self.optimized_write(storage, &key, &value);
        }
    }

    /// Compress storage key using prefix mapping
    pub fn compress_key(&self, key: &str) -> Vec<u8> {
        if !self.compression_enabled {
            return key.as_bytes().to_vec();
        }

        // Find matching prefix
        for (prefix, code) in &self.key_prefix_compression {
            if key.starts_with(prefix) {
                let mut compressed = vec![*code];
                compressed.extend_from_slice(&key.as_bytes()[prefix.len()..]);
                return compressed;
            }
        }

        // No prefix match, return original
        key.as_bytes().to_vec()
    }

    /// Decompress storage key
    pub fn decompress_key(&self, compressed_key: &[u8]) -> String {
        if !self.compression_enabled || compressed_key.is_empty() {
            return String::from_utf8_lossy(compressed_key).to_string();
        }

        let prefix_code = compressed_key[0];

        // Find prefix for this code
        for (prefix, code) in &self.key_prefix_compression {
            if *code == prefix_code {
                let suffix = String::from_utf8_lossy(&compressed_key[1..]);
                return format!("{prefix}{suffix}");
            }
        }

        // No prefix mapping found, return as-is
        String::from_utf8_lossy(compressed_key).to_string()
    }

    /// Clear cache to free memory
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get storage metrics
    pub fn get_metrics(&self) -> &StorageMetrics {
        &self.metrics
    }

    /// Reset metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = StorageMetrics::default();
    }

    /// Get cache hit ratio
    pub fn cache_hit_ratio(&self) -> f64 {
        let total_accesses =
            u128::from(self.metrics.cache_hits) + u128::from(self.metrics.cache_misses);
        if total_accesses > 0 {
            self.metrics.cache_hits as f64 / total_accesses as f64
        } else {
            0.0
        }
    }
}

/// Cache lifetime bound to one exclusive storage borrow.
pub struct StorageSession<'a> {
    optimizer: &'a mut OptimizedStorage,
    storage: &'a mut dyn Storage,
}
impl StorageSession<'_> {
    pub fn read(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.optimizer.read_bound(self.storage, key)
    }
    pub fn write(&mut self, key: &[u8], value: &[u8]) {
        self.optimizer.optimized_write(self.storage, key, value);
    }
    pub fn remove(&mut self, key: &[u8]) {
        self.storage.remove(key);
        self.optimizer.cache.remove(key);
        self.optimizer.metrics.deletes = self.optimizer.metrics.deletes.saturating_add(1);
    }
    pub fn get_metrics(&self) -> &StorageMetrics {
        self.optimizer.get_metrics()
    }
    pub fn load_map<'k, K: PrimaryKey<'k>, T: Serialize + DeserializeOwned>(
        &mut self,
        map: &Map<'k, K, T>,
        key: K,
    ) -> StdResult<T> {
        let bytes = self
            .read(&map.key(key))
            .ok_or_else(|| StdError::not_found(std::any::type_name::<T>()))?;
        from_json(bytes)
    }
    pub fn save_map<'k, K: PrimaryKey<'k>, T: Serialize + DeserializeOwned>(
        &mut self,
        map: &Map<'k, K, T>,
        key: K,
        value: &T,
    ) -> StdResult<()> {
        let bytes = to_json_vec(value)?;
        self.write(&map.key(key), &bytes);
        Ok(())
    }
}
impl Drop for StorageSession<'_> {
    fn drop(&mut self) {
        self.optimizer.clear_cache();
    }
}

/// Efficient key generator for bridge operations
pub struct BridgeKeyGenerator;

impl BridgeKeyGenerator {
    /// Generate optimized key for bridge transaction
    pub fn bridge_transaction_key(bridge_id: &str) -> Vec<u8> {
        let mut key = vec![0x01]; // Bridge transaction prefix
        key.extend_from_slice(bridge_id.as_bytes());
        key
    }

    /// Generate optimized key for token configuration
    pub fn token_config_key(denom: &str) -> Vec<u8> {
        let mut key = vec![0x02]; // Token config prefix
        key.extend_from_slice(denom.as_bytes());
        key
    }

    /// Generate optimized key for validator confirmation
    pub fn validator_confirmation_key(bridge_id: &str, validator: &Addr) -> Vec<u8> {
        let mut key = vec![0x03]; // Validator confirmation prefix
        key.extend_from_slice(bridge_id.as_bytes());
        key.push(0xFF); // Separator
        key.extend_from_slice(validator.as_bytes());
        key
    }

    /// Generate optimized key for batch confirmations
    pub fn batch_confirmation_key(bridge_id: &str) -> Vec<u8> {
        let mut key = vec![0x03, 0xBB]; // Validator confirmation + batch prefix
        key.extend_from_slice(bridge_id.as_bytes());
        key
    }

    /// Generate composite key for range queries
    pub fn composite_key(prefix: &[u8], components: &[&str]) -> Vec<u8> {
        let mut key = prefix.to_vec();
        for (i, component) in components.iter().enumerate() {
            if i > 0 {
                key.push(0xFF); // Separator between components
            }
            key.extend_from_slice(component.as_bytes());
        }
        key
    }
}

/// Efficient serialization utilities
pub struct SerializationOptimizer;

impl SerializationOptimizer {
    /// Serialize with binary encoding for compact storage
    pub fn serialize_compact<T: Serialize>(value: &T) -> StdResult<Vec<u8>> {
        bincode::serialize(value)
            .map_err(|e| StdError::generic_err(format!("Serialization error: {e}")))
    }

    /// Deserialize from binary encoding
    pub fn deserialize_compact<T: DeserializeOwned>(data: &[u8]) -> StdResult<T> {
        bincode::deserialize(data)
            .map_err(|e| StdError::generic_err(format!("Deserialization error: {e}")))
    }

    /// Serialize with JSON for debugging (less efficient)
    pub fn serialize_json<T: Serialize>(value: &T) -> StdResult<Vec<u8>> {
        serde_json::to_vec(value)
            .map_err(|e| StdError::generic_err(format!("JSON serialization error: {e}")))
    }

    /// Deserialize from JSON
    pub fn deserialize_json<T: DeserializeOwned>(data: &[u8]) -> StdResult<T> {
        serde_json::from_slice(data)
            .map_err(|e| StdError::generic_err(format!("JSON deserialization error: {e}")))
    }

    /// Calculate size difference between JSON and binary
    pub fn size_comparison<T: Serialize>(value: &T) -> (usize, usize, f64) {
        let json_size = Self::serialize_json(value).map(|v| v.len()).unwrap_or(0);
        let binary_size = Self::serialize_compact(value).map(|v| v.len()).unwrap_or(0);
        let compression_ratio = if json_size > 0 {
            binary_size as f64 / json_size as f64
        } else {
            1.0
        };
        (json_size, binary_size, compression_ratio)
    }
}

/// Storage access pattern analyzer
pub struct StorageAnalyzer {
    access_patterns: HashMap<String, AccessPattern>,
}

#[derive(Debug, Clone)]
pub struct AccessPattern {
    pub reads: u64,
    pub writes: u64,
    pub first_access: u64,
    pub last_access: u64,
    pub access_frequency: f64,
    pub size_pattern: SizePattern,
}

#[derive(Debug, Clone)]
pub struct SizePattern {
    pub avg_key_size: f64,
    pub avg_value_size: f64,
    pub max_value_size: usize,
    pub min_value_size: usize,
}

impl StorageAnalyzer {
    pub fn new() -> Self {
        Self {
            access_patterns: HashMap::new(),
        }
    }
}

impl Default for StorageAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageAnalyzer {
    /// Record a storage access
    pub fn record_access(
        &mut self,
        key: &str,
        operation: StorageOperation,
        key_size: usize,
        value_size: usize,
    ) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.record_access_at(key, operation, key_size, value_size, timestamp);
    }
    /// Lifetime observation rate with a minimum one-second observation window.
    pub fn record_access_at(
        &mut self,
        key: &str,
        operation: StorageOperation,
        key_size: usize,
        value_size: usize,
        timestamp: u64,
    ) {
        let pattern = self
            .access_patterns
            .entry(key.to_string())
            .or_insert(AccessPattern {
                reads: 0,
                writes: 0,
                first_access: timestamp,
                last_access: timestamp,
                access_frequency: 0.0,
                size_pattern: SizePattern {
                    avg_key_size: key_size as f64,
                    avg_value_size: value_size as f64,
                    max_value_size: value_size,
                    min_value_size: value_size,
                },
            });

        match operation {
            StorageOperation::Read => pattern.reads = pattern.reads.saturating_add(1),
            StorageOperation::Write => pattern.writes = pattern.writes.saturating_add(1),
        }

        // Update size patterns
        let total_accesses = u128::from(pattern.reads) + u128::from(pattern.writes);
        pattern.size_pattern.avg_key_size =
            (pattern.size_pattern.avg_key_size * (total_accesses - 1) as f64 + key_size as f64)
                / total_accesses as f64;
        pattern.size_pattern.avg_value_size =
            (pattern.size_pattern.avg_value_size * (total_accesses - 1) as f64 + value_size as f64)
                / total_accesses as f64;
        pattern.size_pattern.max_value_size = pattern.size_pattern.max_value_size.max(value_size);
        pattern.size_pattern.min_value_size = pattern.size_pattern.min_value_size.min(value_size);

        pattern.last_access = pattern.last_access.max(timestamp);
        let elapsed = (pattern.last_access - pattern.first_access).max(1);
        pattern.access_frequency = total_accesses as f64 / elapsed as f64;
    }

    /// Get optimization recommendations based on access patterns
    pub fn get_recommendations(&self) -> Vec<StorageRecommendation> {
        let mut recommendations = Vec::new();

        for (key, pattern) in &self.access_patterns {
            // Recommend caching for frequently read data
            if u128::from(pattern.reads) > u128::from(pattern.writes) * 3
                && pattern.access_frequency > 0.1
            {
                recommendations.push(StorageRecommendation {
                    key_pattern: key.clone(),
                    recommendation_type: RecommendationType::EnableCaching,
                    description: "High read frequency suggests caching would be beneficial"
                        .to_string(),
                    estimated_gas_savings: pattern.reads.saturating_mul(100), // Estimated savings
                });
            }

            // Recommend batching for frequent writes
            if pattern.writes > 10 && pattern.access_frequency > 0.05 {
                recommendations.push(StorageRecommendation {
                    key_pattern: key.clone(),
                    recommendation_type: RecommendationType::BatchWrites,
                    description: "High write frequency suggests batching would reduce gas costs"
                        .to_string(),
                    estimated_gas_savings: pattern.writes.saturating_mul(50),
                });
            }

            // Recommend compression for large values
            if pattern.size_pattern.avg_value_size > 500.0 {
                recommendations.push(StorageRecommendation {
                    key_pattern: key.clone(),
                    recommendation_type: RecommendationType::CompressValues,
                    description: "Large average value size suggests compression would save storage"
                        .to_string(),
                    estimated_gas_savings: (pattern.size_pattern.avg_value_size * 0.3) as u64,
                });
            }
        }

        recommendations.sort_by(|a, b| b.estimated_gas_savings.cmp(&a.estimated_gas_savings));
        recommendations
    }

    /// Get storage statistics
    pub fn get_statistics(&self) -> StorageStatistics {
        let total_keys = self.access_patterns.len();
        let total_reads: u64 = self
            .access_patterns
            .values()
            .fold(0u64, |total, p| total.saturating_add(p.reads));
        let total_writes: u64 = self
            .access_patterns
            .values()
            .fold(0u64, |total, p| total.saturating_add(p.writes));

        let avg_key_size = if total_keys > 0 {
            self.access_patterns
                .values()
                .map(|p| p.size_pattern.avg_key_size)
                .sum::<f64>()
                / total_keys as f64
        } else {
            0.0
        };

        let avg_value_size = if total_keys > 0 {
            self.access_patterns
                .values()
                .map(|p| p.size_pattern.avg_value_size)
                .sum::<f64>()
                / total_keys as f64
        } else {
            0.0
        };

        StorageStatistics {
            total_keys: total_keys as u64,
            total_reads,
            total_writes,
            avg_key_size,
            avg_value_size,
            read_write_ratio: if total_writes > 0 {
                total_reads as f64 / total_writes as f64
            } else {
                0.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum StorageOperation {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct StorageRecommendation {
    pub key_pattern: String,
    pub recommendation_type: RecommendationType,
    pub description: String,
    pub estimated_gas_savings: u64,
}

#[derive(Debug, Clone)]
pub enum RecommendationType {
    EnableCaching,
    BatchWrites,
    CompressValues,
    OptimizeKeyStructure,
    ReduceStorageAccess,
}

#[derive(Debug, Clone)]
pub struct StorageStatistics {
    pub total_keys: u64,
    pub total_reads: u64,
    pub total_writes: u64,
    pub avg_key_size: f64,
    pub avg_value_size: f64,
    pub read_write_ratio: f64,
}

/// Optimized map wrapper with enhanced features
pub struct OptimizedMap<'a, K, T> {
    inner: Map<'a, K, T>,
    optimizer: OptimizedStorage,
}

impl<'a, K, T> OptimizedMap<'a, K, T>
where
    K: PrimaryKey<'a>,
    T: Serialize + DeserializeOwned,
{
    pub fn new(namespace: &'a str) -> Self {
        Self {
            inner: Map::new(namespace),
            optimizer: OptimizedStorage::new(),
        }
    }

    /// Read the canonical namespaced Map entry. Use a session for cached access.
    pub fn load_optimized(&mut self, storage: &dyn Storage, key: K) -> StdResult<T> {
        let bytes = self
            .optimizer
            .optimized_read(storage, &self.inner.key(key))
            .ok_or_else(|| StdError::not_found(std::any::type_name::<T>()))?;
        from_json(bytes)
    }
    pub fn save_optimized(
        &mut self,
        storage: &mut dyn Storage,
        key: K,
        value: &T,
    ) -> StdResult<()> {
        let bytes = to_json_vec(value)?;
        self.optimizer
            .optimized_write(storage, &self.inner.key(key), &bytes);
        Ok(())
    }
    /// Encode every item before writing. Storage itself does not supply rollback.
    pub fn batch_save(&mut self, storage: &mut dyn Storage, items: Vec<(K, T)>) -> StdResult<()> {
        let mut operations = Vec::with_capacity(items.len());
        for (key, value) in items {
            operations.push((self.inner.key(key).to_vec(), to_json_vec(&value)?));
        }
        self.optimizer.batch_write(storage, operations);
        Ok(())
    }

    /// Get storage metrics
    pub fn get_metrics(&self) -> &StorageMetrics {
        self.optimizer.get_metrics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::MockStorage;

    #[test]
    fn test_optimized_storage_caching() {
        let mut storage = MockStorage::new();
        let mut optimizer = OptimizedStorage::new();
        let mut opt_storage = optimizer.session(&mut storage);

        let key = b"test_key";
        let value = b"test_value";

        // First write
        opt_storage.write(key, value);
        assert_eq!(opt_storage.get_metrics().writes, 1);

        // Write-through populated the session cache.
        let read_value = opt_storage.read(key);
        assert_eq!(read_value, Some(value.to_vec()));
        assert_eq!(opt_storage.get_metrics().cache_misses, 0);

        // Second read (cache hit)
        let read_value2 = opt_storage.read(key);
        assert_eq!(read_value2, Some(value.to_vec()));
        assert_eq!(opt_storage.get_metrics().cache_hits, 2);
    }

    #[test]
    fn test_key_compression() {
        let opt_storage = OptimizedStorage::new();

        let original_key = "bridge_transactions_12345";
        let compressed = opt_storage.compress_key(original_key);
        let decompressed = opt_storage.decompress_key(&compressed);

        assert_eq!(decompressed, original_key);
        assert!(compressed.len() < original_key.len()); // Should be smaller
    }

    #[test]
    fn test_bridge_key_generation() {
        let bridge_key = BridgeKeyGenerator::bridge_transaction_key("bridge_123");
        let token_key = BridgeKeyGenerator::token_config_key("uosmo");
        let validator_key = BridgeKeyGenerator::validator_confirmation_key(
            "bridge_123",
            &Addr::unchecked("validator1"),
        );

        assert!(!bridge_key.is_empty());
        assert!(!token_key.is_empty());
        assert!(!validator_key.is_empty());

        // Keys should have proper prefixes
        assert_eq!(bridge_key[0], 0x01);
        assert_eq!(token_key[0], 0x02);
        assert_eq!(validator_key[0], 0x03);
    }

    #[test]
    fn test_serialization_optimization() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestData {
            id: u64,
            name: String,
            amount: u128,
            active: bool,
        }

        let test_data = TestData {
            id: 12345,
            name: "test_token".to_string(),
            amount: 1000000,
            active: true,
        };

        let compact = SerializationOptimizer::serialize_compact(&test_data).unwrap();
        let json = SerializationOptimizer::serialize_json(&test_data).unwrap();

        // Binary should typically be smaller than JSON
        assert!(compact.len() <= json.len());

        // Roundtrip test
        let deserialized: TestData = SerializationOptimizer::deserialize_compact(&compact).unwrap();
        assert_eq!(deserialized, test_data);

        // Size comparison
        let (json_size, binary_size, ratio) = SerializationOptimizer::size_comparison(&test_data);
        assert!(ratio <= 1.0); // Binary should not be larger than JSON
        assert_eq!(json_size, json.len());
        assert_eq!(binary_size, compact.len());
    }

    #[test]
    fn test_storage_analyzer() {
        let mut analyzer = StorageAnalyzer::new();

        // Record some access patterns
        analyzer.record_access("bridge_tx_1", StorageOperation::Write, 20, 100);
        analyzer.record_access("bridge_tx_1", StorageOperation::Read, 20, 100);
        analyzer.record_access("bridge_tx_1", StorageOperation::Read, 20, 100);
        analyzer.record_access("bridge_tx_1", StorageOperation::Read, 20, 100);
        analyzer.record_access("bridge_tx_1", StorageOperation::Read, 20, 100);

        analyzer.record_access("token_config", StorageOperation::Write, 15, 800);
        analyzer.record_access("token_config", StorageOperation::Write, 15, 850);

        let stats = analyzer.get_statistics();
        assert_eq!(stats.total_keys, 2);
        assert!(stats.total_reads > 0);
        assert!(stats.total_writes > 0);

        let recommendations = analyzer.get_recommendations();
        assert!(!recommendations.is_empty());

        // Should recommend caching for frequently read bridge_tx_1
        let caching_rec = recommendations.iter().find(|r| {
            r.key_pattern == "bridge_tx_1"
                && matches!(r.recommendation_type, RecommendationType::EnableCaching)
        });
        assert!(caching_rec.is_some());
    }

    #[test]
    fn test_batch_operations() {
        let mut storage = MockStorage::new();
        let mut opt_storage = OptimizedStorage::new();

        let operations = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
            (b"key3".to_vec(), b"value3".to_vec()),
        ];

        opt_storage.batch_write(&mut storage, operations);
        assert_eq!(opt_storage.get_metrics().writes, 3);

        // Verify all values were written
        assert_eq!(
            opt_storage.optimized_read(&storage, b"key1"),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            opt_storage.optimized_read(&storage, b"key2"),
            Some(b"value2".to_vec())
        );
        assert_eq!(
            opt_storage.optimized_read(&storage, b"key3"),
            Some(b"value3".to_vec())
        );
    }

    #[test]
    fn full_cache_updates_existing_values_and_removes_deleted_values() {
        let mut storage = MockStorage::new();
        let mut optimizer = OptimizedStorage::with_config(1, false);
        let mut session = optimizer.session(&mut storage);
        session.write(b"one", b"old");
        session.write(b"two", b"uncached");
        session.write(b"one", b"new");
        assert_eq!(session.read(b"one"), Some(b"new".to_vec()));
        assert_eq!(session.read(b"two"), Some(b"uncached".to_vec()));
        session.remove(b"one");
        assert_eq!(session.read(b"one"), None);
        assert_eq!(session.get_metrics().deletes, 1);
    }

    #[test]
    fn unbound_access_and_new_sessions_do_not_reuse_stale_values() {
        let mut first = MockStorage::new();
        let mut second = MockStorage::new();
        first.set(b"same", b"first");
        second.set(b"same", b"second");
        let mut optimizer = OptimizedStorage::new();
        assert_eq!(
            optimizer.optimized_read(&first, b"same"),
            Some(b"first".to_vec())
        );
        assert_eq!(
            optimizer.optimized_read(&second, b"same"),
            Some(b"second".to_vec())
        );
        {
            let mut session = optimizer.session(&mut first);
            assert_eq!(session.read(b"same"), Some(b"first".to_vec()));
        }
        first.set(b"same", b"changed");
        assert_eq!(
            optimizer.session(&mut first).read(b"same"),
            Some(b"changed".to_vec())
        );
        first.remove(b"same");
        assert_eq!(optimizer.optimized_read(&first, b"same"), None);
    }

    struct CountedStorage {
        inner: MockStorage,
        gets: std::cell::Cell<usize>,
    }
    impl Storage for CountedStorage {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.gets.set(self.gets.get() + 1);
            self.inner.get(key)
        }
        fn set(&mut self, key: &[u8], value: &[u8]) {
            self.inner.set(key, value);
        }
        fn remove(&mut self, key: &[u8]) {
            self.inner.remove(key);
        }
        fn range<'a>(
            &'a self,
            start: Option<&[u8]>,
            end: Option<&[u8]>,
            order: cosmwasm_std::Order,
        ) -> Box<dyn Iterator<Item = cosmwasm_std::Record> + 'a> {
            self.inner.range(start, end, order)
        }
    }

    #[test]
    fn bound_cache_avoids_backing_reads_and_zero_capacity_disables_it() {
        for capacity in [0, 1] {
            let mut storage = CountedStorage {
                inner: MockStorage::new(),
                gets: std::cell::Cell::new(0),
            };
            storage.set(b"key", b"value");
            let mut optimizer = OptimizedStorage::with_config(capacity, false);
            {
                let mut session = optimizer.session(&mut storage);
                for _ in 0..3 {
                    assert_eq!(session.read(b"key"), Some(b"value".to_vec()));
                }
                assert_eq!(
                    session.get_metrics().cache_hits,
                    if capacity == 0 { 0 } else { 2 }
                );
            }
            assert_eq!(storage.gets.get(), if capacity == 0 { 3 } else { 1 });
        }
    }

    #[test]
    fn optimized_maps_use_canonical_namespaces_and_regular_map_encoding() {
        let mut storage = MockStorage::new();
        let mut left = OptimizedMap::<&str, u64>::new("left");
        let mut right = OptimizedMap::<&str, u64>::new("right");
        left.save_optimized(&mut storage, "same", &11).unwrap();
        right
            .batch_save(&mut storage, vec![("same", 22), ("other", 33)])
            .unwrap();
        assert_eq!(
            Map::<&str, u64>::new("left")
                .load(&storage, "same")
                .unwrap(),
            11
        );
        assert_eq!(
            Map::<&str, u64>::new("right")
                .load(&storage, "same")
                .unwrap(),
            22
        );
        assert!(storage.get(b"same").is_none());
        Map::<&str, u64>::new("left")
            .save(&mut storage, "same", &44)
            .unwrap();
        assert_eq!(left.load_optimized(&storage, "same").unwrap(), 44);
        assert_eq!(right.load_optimized(&storage, "same").unwrap(), 22);
        assert_eq!(left.get_metrics().writes, 1);
        assert_eq!(right.get_metrics().writes, 2);
    }

    #[test]
    fn session_caches_namespaced_map_entries_separately() {
        let mut storage = MockStorage::new();
        let mut optimizer = OptimizedStorage::new();
        let left = Map::<&str, u64>::new("left");
        let right = Map::<&str, u64>::new("right");
        {
            let mut session = optimizer.session(&mut storage);
            session.save_map(&left, "same", &10).unwrap();
            session.save_map(&right, "same", &20).unwrap();
            assert_eq!(session.load_map(&left, "same").unwrap(), 10);
            assert_eq!(session.load_map(&right, "same").unwrap(), 20);
        }
        assert_eq!(left.load(&storage, "same").unwrap(), 10);
        assert_eq!(right.load(&storage, "same").unwrap(), 20);
    }

    #[derive(Deserialize)]
    struct EncodeCheck(u8);
    impl Serialize for EncodeCheck {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if self.0 == 0 {
                return Err(serde::ser::Error::custom("fixture encoding failure"));
            }
            serializer.serialize_u8(self.0)
        }
    }
    #[test]
    fn batch_encoding_failure_does_not_write_earlier_items() {
        let mut storage = MockStorage::new();
        let mut map = OptimizedMap::<&str, EncodeCheck>::new("batch");
        assert!(map
            .batch_save(
                &mut storage,
                vec![("first", EncodeCheck(1)), ("second", EncodeCheck(0))]
            )
            .is_err());
        assert!(Map::<&str, u8>::new("batch")
            .may_load(&storage, "first")
            .unwrap()
            .is_none());
        assert_eq!(map.get_metrics().writes, 0);
    }

    #[test]
    fn analyzer_uses_full_observation_window_and_handles_clock_reversal() {
        let mut analyzer = StorageAnalyzer::new();
        for time in [100, 105, 120] {
            analyzer.record_access_at("key", StorageOperation::Read, 3, 10, time);
        }
        assert!((analyzer.access_patterns["key"].access_frequency - 0.15).abs() < 1e-12);
        analyzer.record_access_at("key", StorageOperation::Read, 3, 10, 95);
        assert_eq!(analyzer.access_patterns["key"].last_access, 120);
        assert!((analyzer.access_patterns["key"].access_frequency - 0.2).abs() < 1e-12);
    }

    #[test]
    fn analyzer_respects_strict_read_ratio_threshold() {
        let mut analyzer = StorageAnalyzer::new();
        analyzer.record_access_at("key", StorageOperation::Write, 3, 10, 100);
        for _ in 0..3 {
            analyzer.record_access_at("key", StorageOperation::Read, 3, 10, 100);
        }
        assert!(analyzer.get_recommendations().is_empty());
        analyzer.record_access_at("key", StorageOperation::Read, 3, 10, 100);
        assert!(analyzer
            .get_recommendations()
            .iter()
            .any(|r| matches!(r.recommendation_type, RecommendationType::EnableCaching)));
    }
}
