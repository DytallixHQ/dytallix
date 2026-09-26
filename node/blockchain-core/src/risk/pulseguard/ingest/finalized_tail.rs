use crate::risk::pulseguard::{now_ms, RiskEvent};
use crate::types::{Block, Transaction};
use log::{debug, trace};
use serde_json::json;
use tokio::sync::mpsc::Sender;

pub async fn stream_finalized(tx: Sender<RiskEvent>, blocks: Vec<Block>) {
    for b in blocks {
        for t in &b.transactions {
            let (to_addr, amount_u128) = match t {
                Transaction::Transfer(tr) => (tr.to.clone(), tr.amount),
                Transaction::Call(c) => (c.to.clone(), c.value),
                Transaction::Stake(s) => (s.validator.clone(), s.amount),
                Transaction::Deploy(_) => ("".to_string(), 0u128),
                Transaction::AIRequest(_) => ("".to_string(), 0u128),
            };
            let ev = RiskEvent {
                tx_hash: t.hash(),
                from: t.from().clone(),
                to: to_addr,
                amount: amount_u128,
                timestamp: (now_ms() / 1000) as u64,
                metadata: enrich_finalized(b.header.number),
            };
            if tx.send(ev).await.is_err() {
                debug!("finalized channel closed");
                return;
            }
            trace!("finalized event queued");
        }
    }
}

fn enrich_finalized(height: u64) -> serde_json::Value {
    json!({"source":"finalized","height":height,"settled":true})
}
