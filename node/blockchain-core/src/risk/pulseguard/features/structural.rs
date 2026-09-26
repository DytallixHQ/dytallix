use crate::risk::pulseguard::graph::dag::DynamicDag;
use crate::risk::pulseguard::{FeatureVector, RiskEvent};
pub struct StructuralEngine<'a> {
    pub dag: &'a DynamicDag,
}
impl<'a> StructuralEngine<'a> {
    pub fn extract(&self, ev: &RiskEvent) -> Option<FeatureVector> {
        let fanout = self.dag.fanout_k(&ev.from, 3) as f32;
        Some(FeatureVector::new(
            ev.tx_hash.clone(),
            vec!["fanout_k3".into()],
            vec![fanout],
        ))
    }
}
