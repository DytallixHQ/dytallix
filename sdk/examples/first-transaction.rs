//! Send your first transaction on the Dytallix testnet.
//! Run with: cargo run -p dytallix-sdk --features network --example first-transaction

use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::error::SdkError;
use dytallix_sdk::faucet::FaucetClient;
use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::Token;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let sender_keypair = DytallixKeypair::generate();
    let sender_addr = dytallix_core::address::DAddr::from_public_key(sender_keypair.public_key())?;
    let recipient_keypair = DytallixKeypair::generate();
    let recipient_addr =
        dytallix_core::address::DAddr::from_public_key(recipient_keypair.public_key())?;
    println!("Sender: {sender_addr}");
    println!("Recipient: {recipient_addr}");

    let faucet = FaucetClient::testnet();
    let balance = faucet.fund(&sender_addr).await?;
    println!("{balance}");

    let client = DytallixClient::testnet().await?;
    let account = client.get_account(&sender_addr).await?;
    let tx = TransactionBuilder::new()
        .from(sender_addr.clone())
        .to(recipient_addr.clone())
        .amount(1, Token::DRT)
        .nonce(account.nonce)
        .build()?;
    let (tx, fee) = tx
        .with_estimated_fee(&client)
        .await
        .map_err(humanize_transaction_error)?;
    println!("{fee}");

    let signed = tx.sign(&sender_keypair)?;
    let receipt = client
        .submit_transaction(&signed)
        .await
        .map_err(humanize_transaction_error)?;
    println!("Transaction: {}", receipt.hash);
    let receipt = wait_for_receipt(&client, &receipt.hash).await?;
    println!("Status: {:?}", receipt.status);

    let recipient_account = client.get_account(&recipient_addr).await?;
    println!("Recipient balance: {}", recipient_account.balance);
    Ok(())
}

fn humanize_transaction_error(error: SdkError) -> anyhow::Error {
    anyhow::anyhow!(error.to_string())
}

async fn wait_for_receipt(
    client: &DytallixClient,
    hash: &str,
) -> anyhow::Result<dytallix_sdk::TransactionReceipt> {
    for _ in 0..15 {
        let receipt = client.get_transaction(hash).await?;
        match receipt.status {
            dytallix_sdk::TransactionStatus::Pending => sleep(Duration::from_secs(1)).await,
            _ => return Ok(receipt),
        }
    }
    client.get_transaction(hash).await.map_err(Into::into)
}
