// Allow future async trait lint if introduced and silence currently discussed lints
#![allow(clippy::module_name_repetitions)]

use crate::risk::pulseguard::alerts::queue::{AlertQueue, StdoutSink};
use crate::risk::pulseguard::features::structural::StructuralEngine;
use crate::risk::pulseguard::features::temporal::TemporalWindowEngine;
use crate::risk::pulseguard::graph::dag::DynamicDag;
use crate::risk::pulseguard::models::ensemble::Ensemble;
use crate::risk::pulseguard::RiskScore;
use crate::risk::pulseguard::{FeatureVector, RiskEvent};
use log::info;
use tokio::sync::{broadcast, mpsc};

pub struct PulseGuardEngine {
    pub event_tx: mpsc::Sender<RiskEvent>,
    pub alert_tx: broadcast::Sender<RiskScore>,
}

impl PulseGuardEngine {
    pub fn new(buffer: usize) -> (Self, tokio::task::JoinHandle<()>) {
        let (event_tx, mut event_rx) = mpsc::channel::<RiskEvent>(buffer);
        let (alert_tx, _) = broadcast::channel::<RiskScore>(1024);
        let alert_tx_clone = alert_tx.clone();
        let handle = tokio::spawn(async move {
            let mut temporal = TemporalWindowEngine::new();
            let mut dag = DynamicDag::default();
            let ensemble = Ensemble::new();
            let mut alerts = AlertQueue::default();
            let sink = StdoutSink;
            while let Some(ev) = event_rx.recv().await {
                // update graph with a placeholder edge (self loop minimal) - real impl: decode tx type
                dag.add_edge(
                    ev.from.clone(),
                    crate::risk::pulseguard::graph::dag::Edge {
                        to: ev.to.clone(),
                        edge_type: crate::risk::pulseguard::graph::dag::EdgeType::Transfer,
                        timestamp: ev.timestamp,
                        amount: ev.amount,
                    },
                );

                let fv_t = temporal.ingest(ev.clone());
                let structural = StructuralEngine { dag: &dag };
                let fv = merge_features(fv_t, structural.extract(&ev));
                let rs = ensemble.infer_with_graph(&fv, &dag, &ev.from);
                if rs.score > 80.0 {
                    alerts.push(rs.clone());
                    let _ = alert_tx_clone.send(rs.clone());
                }
                alerts.drain_to_sink(&sink, 5);
            }
            info!("PulseGuard engine loop terminated");
        });
        (Self { event_tx, alert_tx }, handle)
    }
}

fn merge_features(mut base: FeatureVector, extra: Option<FeatureVector>) -> FeatureVector {
    if let Some(e) = extra {
        for (n, v) in e.feature_names.into_iter().zip(e.features.into_iter()) {
            base.feature_names.push(n);
            base.features.push(v);
        }
    }
    base
}
