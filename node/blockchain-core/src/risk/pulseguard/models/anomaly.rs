use crate::risk::pulseguard::FeatureVector;

/// Simple anomaly scoring model.
/// Currently computes the mean of the feature vector (robust placeholder)
/// and normalizes it into the [0,1] range by dividing by 10 and capping.
#[derive(Debug, Default, Copy, Clone)]
pub struct AnomalyModel;

impl AnomalyModel {
    pub fn new() -> Self {
        Self
    }

    pub fn score(&self, fv: &FeatureVector) -> f32 {
        if fv.features.is_empty() {
            return 0.0;
        }
        let sum: f32 = fv.features.iter().copied().sum();
        let mean = sum / fv.features.len() as f32;
        (mean / 10.0).clamp(0.0, 1.0)
    }
}
