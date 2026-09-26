use crate::risk::pulseguard::models::ensemble::Ensemble;
use crate::risk::pulseguard::FeatureVector;
use std::time::Instant;

pub fn quick_bench() -> (f32, f32, f32) {
    let ensemble = Ensemble::new();
    let mut samples = Vec::new();
    for i in 0..500 {
        let fv = FeatureVector::new(
            format!("tx{i}"),
            vec![
                "velocity_1m".into(),
                "velocity_5m".into(),
                "velocity_60m".into(),
            ],
            vec![i as f32, (i / 2) as f32, (i / 10) as f32],
        );
        let start = Instant::now();
        let _ = ensemble.infer(&fv);
        let dur = start.elapsed().as_micros() as f32 / 1000.0;
        samples.push(dur);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples[(samples.len() * 50) / 100];
    let p95 = samples[(samples.len() * 95) / 100];
    let p99 = samples[(samples.len() * 99) / 100];
    (p50, p95, p99)
}
