use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Gossip configuration constants
pub const DEFAULT_MEMPOOL_SEEN_TTL_MS: u64 = 300_000; // 5 minutes
pub const DEFAULT_GOSSIP_MAX_OUTBOUND: usize = 1000;
pub const DEFAULT_GOSSIP_THROTTLE_INTERVAL_MS: u64 = 100; // 100ms between batches

/// Configuration for gossip protocol
#[derive(Debug, Clone)]
pub struct GossipConfig {
    pub seen_ttl_ms: u64,
    pub max_outbound_per_peer: usize,
    pub throttle_interval_ms: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            seen_ttl_ms: std::env::var("DYT_MEMPOOL_SEEN_TTL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MEMPOOL_SEEN_TTL_MS),
            max_outbound_per_peer: std::env::var("DYT_GOSSIP_MAX_OUTBOUND")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_GOSSIP_MAX_OUTBOUND),
            throttle_interval_ms: DEFAULT_GOSSIP_THROTTLE_INTERVAL_MS,
        }
    }
}

/// Seen cache entry with TTL
#[derive(Debug, Clone)]
struct SeenEntry {
    #[allow(dead_code)]
    // hash is redundant given HashMap key but kept for potential debugging / metrics
    tx_hash: String,
    seen_at: u64,
    from_peers: HashSet<String>,
}

/// Peer outbound queue with throttling
#[derive(Debug)]
struct PeerQueue {
    #[allow(dead_code)] // retained for potential logging / metrics
    peer_id: String,
    pending_txs: VecDeque<String>, // Transaction hashes
    last_sent: u64,
}

impl PeerQueue {
    fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            pending_txs: VecDeque::new(),
            last_sent: 0,
        }
    }

    fn can_send(&self, throttle_interval_ms: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        now - self.last_sent >= throttle_interval_ms
    }

    fn mark_sent(&mut self) {
        self.last_sent = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
    }
}

/// Transaction gossip manager with duplicate suppression and throttling
pub struct TransactionGossip {
    config: GossipConfig,
    /// Cache of seen transaction hashes with TTL
    seen_cache: Arc<RwLock<HashMap<String, SeenEntry>>>,
    /// Per-peer outbound queues
    peer_queues: Arc<RwLock<HashMap<String, PeerQueue>>>,
    /// Broadcast tracking to avoid duplicate broadcasts
    broadcast_hashes: Arc<RwLock<HashSet<String>>>,
}

impl TransactionGossip {
    pub fn new() -> Self {
        Self::with_config(GossipConfig::default())
    }

    pub fn with_config(config: GossipConfig) -> Self {
        Self {
            config,
            seen_cache: Arc::new(RwLock::new(HashMap::new())),
            peer_queues: Arc::new(RwLock::new(HashMap::new())),
            broadcast_hashes: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Check if we should gossip this transaction (not seen recently from enough peers)
    pub fn should_gossip(&self, tx_hash: &str, from_peer: Option<&str>) -> bool {
        let mut seen_cache = self.seen_cache.write().unwrap();

        // Clean up expired entries
        self.cleanup_expired_entries(&mut seen_cache);

        let _now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        match seen_cache.get_mut(tx_hash) {
            Some(entry) => {
                // Update seen entry
                if let Some(peer) = from_peer {
                    entry.from_peers.insert(peer.to_string());
                }
                false // Already seen, don't gossip
            }
            None => {
                // New transaction, add to seen cache
                let mut from_peers = HashSet::new();
                if let Some(peer) = from_peer {
                    from_peers.insert(peer.to_string());
                }

                seen_cache.insert(
                    tx_hash.to_string(),
                    SeenEntry {
                        tx_hash: tx_hash.to_string(),
                        seen_at: _now,
                        from_peers,
                    },
                );

                true // New transaction, should gossip
            }
        }
    }

    /// Mark transaction as broadcast to avoid rebroadcast
    pub fn mark_broadcast(&self, tx_hash: &str) {
        let mut broadcast_hashes = self.broadcast_hashes.write().unwrap();
        broadcast_hashes.insert(tx_hash.to_string());
    }

    /// Check if transaction was already broadcast
    pub fn was_broadcast(&self, tx_hash: &str) -> bool {
        let broadcast_hashes = self.broadcast_hashes.read().unwrap();
        broadcast_hashes.contains(tx_hash)
    }

    /// Add transaction to peer queues for gossip
    pub fn queue_for_gossip(&self, tx_hash: &str, peers: &[String]) {
        if self.was_broadcast(tx_hash) {
            return; // Skip rebroadcast
        }

        let mut peer_queues = self.peer_queues.write().unwrap();

        for peer_id in peers {
            let queue = peer_queues
                .entry(peer_id.clone())
                .or_insert_with(|| PeerQueue::new(peer_id.clone()));

            // Add to queue if not at capacity
            if queue.pending_txs.len() < self.config.max_outbound_per_peer {
                queue.pending_txs.push_back(tx_hash.to_string());
            }
        }

        // Mark as broadcast
        self.mark_broadcast(tx_hash);
    }

    /// Get next batch of transactions to send to a peer (with throttling)
    pub fn get_pending_for_peer(&self, peer_id: &str, batch_size: usize) -> Vec<String> {
        let mut peer_queues = self.peer_queues.write().unwrap();

        if let Some(queue) = peer_queues.get_mut(peer_id) {
            if queue.can_send(self.config.throttle_interval_ms) {
                let mut batch = Vec::new();
                for _ in 0..batch_size {
                    if let Some(tx_hash) = queue.pending_txs.pop_front() {
                        batch.push(tx_hash);
                    } else {
                        break;
                    }
                }

                if !batch.is_empty() {
                    queue.mark_sent();
                }

                return batch;
            }
        }

        Vec::new()
    }

    /// Get statistics for monitoring
    pub fn get_stats(&self) -> GossipStats {
        let seen_cache = self.seen_cache.read().unwrap();
        let peer_queues = self.peer_queues.read().unwrap();
        let broadcast_hashes = self.broadcast_hashes.read().unwrap();

        let total_pending = peer_queues.values().map(|q| q.pending_txs.len()).sum();

        GossipStats {
            seen_cache_size: seen_cache.len(),
            total_pending_gossip: total_pending,
            active_peers: peer_queues.len(),
            total_broadcast: broadcast_hashes.len(),
        }
    }

    /// Clean up expired entries from seen cache
    fn cleanup_expired_entries(&self, seen_cache: &mut HashMap<String, SeenEntry>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        seen_cache.retain(|_, entry| now - entry.seen_at < self.config.seen_ttl_ms);
    }

    /// Periodic cleanup task (should be called regularly)
    pub fn cleanup(&self) {
        let mut seen_cache = self.seen_cache.write().unwrap();
        self.cleanup_expired_entries(&mut seen_cache);

        // Also cleanup old broadcast hashes (keep them for TTL duration)
        let mut broadcast_hashes = self.broadcast_hashes.write().unwrap();
        let _now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // For simplicity, clear all broadcast hashes older than TTL
        // In production, you'd want to track timestamps per hash
        broadcast_hashes.clear();
    }
}

impl Default for TransactionGossip {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for monitoring gossip behavior
#[derive(Debug, Clone)]
pub struct GossipStats {
    pub seen_cache_size: usize,
    pub total_pending_gossip: usize,
    pub active_peers: usize,
    pub total_broadcast: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gossip_duplicate_suppression() {
        let gossip = TransactionGossip::new();
        let tx_hash = "test_hash";

        // First time should gossip
        assert!(gossip.should_gossip(tx_hash, Some("peer1")));

        // Second time should not gossip
        assert!(!gossip.should_gossip(tx_hash, Some("peer2")));
    }

    #[test]
    fn test_peer_queue_throttling() {
        let mut queue = PeerQueue::new("peer1".to_string());

        // Should be able to send initially
        assert!(queue.can_send(100));

        // Mark as sent
        queue.mark_sent();

        // Should not be able to send immediately
        assert!(!queue.can_send(100));
    }

    #[test]
    fn test_broadcast_tracking() {
        let gossip = TransactionGossip::new();
        let tx_hash = "test_hash";

        // Initially not broadcast
        assert!(!gossip.was_broadcast(tx_hash));

        // Mark as broadcast
        gossip.mark_broadcast(tx_hash);

        // Now should be marked as broadcast
        assert!(gossip.was_broadcast(tx_hash));
    }
}
