use super::{anomaly::AnomalyModel, gbm::GbmModel};
use crate::risk::pulseguard::explain::paths::add_path_from_dag;
use crate::risk::pulseguard::graph::dag::DynamicDag;
use crate::risk::pulseguard::{FeatureVector, RiskScore};

pub struct Ensemble {
    pub gbm: GbmModel,
    pub anomaly: AnomalyModel,
}
impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}

impl Ensemble {
    pub fn new() -> Self {
        Self {
            gbm: GbmModel::load_stub(),
            anomaly: AnomalyModel::new(),
        }
    }
    pub fn infer(&self, fv: &FeatureVector) -> RiskScore {
        let mut rs = RiskScore::new(fv.tx_hash.clone());
        let g = self.gbm.predict(fv);
        let a = self.anomaly.score(fv);
        rs.score = (0.85 * g + 15.0 * a).min(100.0);
        rs.confidence = 0.7 + 0.2 * a;
        if rs.score > 80.0 {
            rs.reasons.push("high_risk_pattern".into());
        } else {
            rs.reasons.push("baseline".into());
        }
        rs.top_features = fv
            .feature_names
            .iter()
            .cloned()
            .zip(fv.features.iter().copied())
            .take(5)
            .collect();
        rs
    }
    pub fn infer_with_graph(&self, fv: &FeatureVector, dag: &DynamicDag, start: &str) -> RiskScore {
        let mut rs = self.infer(fv);
        add_path_from_dag(&mut rs, dag, start);
        rs
    }
}
