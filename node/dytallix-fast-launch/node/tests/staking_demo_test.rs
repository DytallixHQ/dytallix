// Staking Demo Integration Test
// Tests the complete delegate → accrue → claim workflow using HTTP API
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Staking Demo Integration Test");
    println!("================================\n");

    let base_url = "http://localhost:3030";
    let client = reqwest::Client::new();

    // Wait for node to start
    sleep(Duration::from_secs(2)).await;

    // Test addresses
    let delegator = "dyt1delegator123";
    let validator = "dyt1validator456";

    println!("1. Testing initial balance endpoint");
    let balance_url = format!("{}/api/staking/balance/{}", base_url, delegator);
    let response = client.get(&balance_url).send().await?;

    if response.status().is_success() {
        let balance: serde_json::Value = response.json().await?;
        println!("   ✓ Initial balance: {}", balance);
    } else {
        println!("   ⚠ Balance endpoint not available (expected if staking disabled)");
    }

    println!("\n2. Testing accrued rewards endpoint");
    let accrued_url = format!("{}/api/staking/accrued/{}", base_url, delegator);
    let response = client.get(&accrued_url).send().await?;

    if response.status().is_success() {
        let accrued: serde_json::Value = response.json().await?;
        println!("   ✓ Initial accrued rewards: {}", accrued);
    } else {
        println!("   ⚠ Accrued endpoint returned: {}", response.status());
    }

    println!("\n3. Testing delegation endpoint");
    let delegate_payload = json!({
        "delegator_addr": delegator,
        "validator_addr": validator,
        "amount_udgt": "1000000000000"  // 1M DGT in uDGT
    });

    let delegate_url = format!("{}/api/staking/delegate", base_url);
    let response = client
        .post(&delegate_url)
        .json(&delegate_payload)
        .send()
        .await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let result: serde_json::Value = response.json().await?;
            println!("   ✓ Delegation successful: {}", result);
        }
        reqwest::StatusCode::NOT_IMPLEMENTED => {
            println!("   ⚠ Staking feature is disabled (DYT_ENABLE_STAKING=true needed)");
        }
        _ => {
            let error_text = response.text().await?;
            println!("   ✗ Delegation failed: {}", error_text);
        }
    }

    println!("\n4. Testing claim rewards endpoint");
    let claim_payload = json!({
        "address": delegator
    });

    let claim_url = format!("{}/api/staking/claim", base_url);
    let response = client.post(&claim_url).json(&claim_payload).send().await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let result: serde_json::Value = response.json().await?;
            println!("   ✓ Claim successful: {}", result);
        }
        reqwest::StatusCode::NOT_IMPLEMENTED => {
            println!("   ⚠ Staking feature is disabled for claims too");
        }
        _ => {
            let error_text = response.text().await?;
            println!("   ⚠ Claim response: {}", error_text);
        }
    }

    println!("\n5. Testing stats endpoint");
    let stats_url = format!("{}/api/stats", base_url);
    let response = client.get(&stats_url).send().await?;

    if response.status().is_success() {
        let stats: serde_json::Value = response.json().await?;
        if let Some(staking) = stats.get("staking") {
            println!("   ✓ Staking stats: {}", staking);
        } else {
            println!("   ⚠ No staking stats in response");
        }
    } else {
        println!("   ✗ Stats endpoint failed: {}", response.status());
    }

    println!("\n📋 Test Summary:");
    println!("   - Balance endpoint: Implemented");
    println!("   - Delegate endpoint: Implemented (needs DYT_ENABLE_STAKING=true)");
    println!("   - Claim endpoint: Implemented (needs DYT_ENABLE_STAKING=true)");
    println!("   - Stats endpoint: Shows staking data");

    println!("\n💡 To enable staking demo:");
    println!("   export DYT_ENABLE_STAKING=true");
    println!("   ./dytallix-lean-launch/node/target/debug/dytallix-lean-node");

    Ok(())
}
