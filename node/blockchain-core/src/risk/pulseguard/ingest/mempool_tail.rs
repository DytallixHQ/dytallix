use crate::risk::pulseguard::now_ms;
use crate::risk::pulseguard::RiskEvent;
use crate::types::Transaction;
use log::{debug, trace};
use serde_json::json;
use tokio::sync::mpsc::Sender;

pub async fn stream_mempool(tx: Sender<RiskEvent>, mem: Vec<Transaction>) {
    for t in mem {
        let enriched = enrich(&t);
        let (to_addr, amount_u128) = match &t {
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
            metadata: enriched,
        };
        if tx.send(ev).await.is_err() {
            debug!("mempool channel closed");
            break;
        }
        trace!("mempool event queued");
    }
}

fn enrich(t: &Transaction) -> serde_json::Value {
    // placeholder enrichment (address tags, clusters, historical stats)
    let _ = t; // suppress unused warning until real enrichment
    json!({
        "source":"mempool",
        "from_cluster":"c_stub",
        "to_cluster":"c_stub",
        "risk_tags":["new_address"],
        "historical_tx_count": 0
    })
}
