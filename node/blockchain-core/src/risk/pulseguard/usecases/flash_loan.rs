use crate::risk::pulseguard::FeatureVector;
pub fn detect_flash_loan(fv: &FeatureVector) -> bool {
    fv.features.iter().any(|v| *v > 500.0)
}
