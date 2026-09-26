use crate::risk::pulseguard::RiskScore;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AlertItem(pub RiskScore);
impl Eq for AlertItem {}
impl PartialEq for AlertItem {
    fn eq(&self, other: &Self) -> bool {
        self.0.score == other.0.score
    }
}
impl PartialOrd for AlertItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for AlertItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .score
            .partial_cmp(&other.0.score)
            .unwrap_or(Ordering::Equal)
    }
}

pub trait AlertSink: Send + Sync {
    fn send(&self, alert: &RiskScore);
}

pub struct StdoutSink;
impl AlertSink for StdoutSink {
    fn send(&self, alert: &RiskScore) {
        println!("ALERT score={} tx={}", alert.score, alert.tx_hash);
    }
}

#[derive(Default)]
pub struct AlertQueue {
    heap: BinaryHeap<AlertItem>,
    last_emit: HashMap<String, Instant>,
}
impl AlertQueue {
    pub fn push(&mut self, rs: RiskScore) {
        // simple suppression if emitted within last 5s
        if let Some(ts) = self.last_emit.get(&rs.tx_hash) {
            if ts.elapsed() < Duration::from_secs(5) {
                return;
            }
        }
        self.last_emit.insert(rs.tx_hash.clone(), Instant::now());
        self.heap.push(AlertItem(rs));
    }
    pub fn pop(&mut self) -> Option<RiskScore> {
        self.heap.pop().map(|i| i.0)
    }
    pub fn drain_to_sink<S: AlertSink>(&mut self, sink: &S, max: usize) {
        for _ in 0..max {
            if let Some(a) = self.pop() {
                sink.send(&a);
            } else {
                break;
            }
        }
    }
}
