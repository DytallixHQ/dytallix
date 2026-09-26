use crate::risk::pulseguard::FeatureVector;
pub fn simple_attributions(fv: &FeatureVector) -> Vec<(String, f32)> {
    let total: f32 = fv.features.iter().sum();
    fv.feature_names
        .iter()
        .cloned()
        .zip(
            fv.features
                .iter()
                .map(|v| if total > 0.0 { v / total } else { 0.0 }),
        )
        .collect()
}
