//! Check an operator-selected service. This reports HTTP status only.
use anyhow::{bail, Result};
use dytallix_node::consensus::{AIOracleClient, AIServiceConfig};

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let mut client = AIOracleClient::new(AIServiceConfig {
        base_url,
        ..AIServiceConfig::default()
    });
    client.set_timeout(5);
    if !client.health_check().await? {
        bail!("Health endpoint returned a non-success HTTP status");
    }
    println!("Health endpoint returned a successful HTTP status.");
    println!("This check does not validate oracle signatures or analysis results.");
    Ok(())
}
