//! Inspect HTTP circuit state against an operator-selected service.
//! The settings below are examples, not an approved production policy.
use dytallix_node::consensus::http_circuit_breaker::HttpCircuitBreakerConfig;
use dytallix_node::consensus::{AIOracleClient, AIServiceConfig};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let client = AIOracleClient::with_circuit_breaker(
        AIServiceConfig {
            base_url,
            timeout_seconds: 2,
            max_retries: 1,
            ..AIServiceConfig::default()
        },
        HttpCircuitBreakerConfig {
            failure_threshold_bps: 5000,
            min_requests: 2,
            window_size: 4,
            recovery_timeout: Duration::from_secs(30),
        },
    )?;
    for attempt in 1..=3 {
        println!(
            "Health request {attempt}: {:?}",
            client.health_check().await
        );
        println!("Circuit: {:?}", client.circuit_breaker_status()?);
    }
    println!("A successful HTTP status does not establish oracle authenticity.");
    Ok(())
}
