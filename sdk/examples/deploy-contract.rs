//! Deploy your first WASM smart contract on the Dytallix testnet.
//! Run with: cargo run -p dytallix-cli --example deploy-contract
//! Run dytallix init first if you have not already.

use dytallix_sdk::keystore::Keystore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let keystore = Keystore::open(Keystore::default_path())?;
    let entry = keystore.active().ok_or_else(|| {
        anyhow::anyhow!(
            "No active wallet. Run dytallix init first.\nDiscord: https://discord.gg/eyVvu5kmPG"
        )
    })?;
    println!("Deploying from: {}", entry.address);
    println!("Run: dytallix contract deploy ./my_contract.wasm");
    println!("See: https://dytallix.com/docs/getting-started");
    Ok(())
}
