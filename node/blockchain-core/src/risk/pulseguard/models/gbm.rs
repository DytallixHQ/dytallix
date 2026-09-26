use crate::risk::pulseguard::FeatureVector;
pub struct GbmModel {
    pub weights: Vec<f32>,
}
impl GbmModel {
    pub fn load_stub() -> Self {
        Self {
            weights: vec![0.3, 0.4, 0.3],
        }
    }
    pub fn predict(&self, fv: &FeatureVector) -> f32 {
        fv.features
            .iter()
            .zip(self.weights.iter().cycle())
            .map(|(a, w)| a * w)
            .sum::<f32>()
            .min(100.0)
    }
}
