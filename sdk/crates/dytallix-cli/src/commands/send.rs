//! Send command implementation.

use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::{Args, ValueEnum};
use tokio::time::sleep;

use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::Token;

use crate::commands::{
    active_entry, active_keypair, configured_client, format_micro_amount, format_number,
    humanize_sdk_error, load_keystore, validate_address,
};
use crate::output;

const MICROS_PER_TOKEN: u128 = 1_000_000;
const SEND_CONFIRMATION_POLL_COUNT: usize = 15;
const SEND_CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Arguments for the `send` command.
#[derive(Debug, Clone, Args)]
pub struct SendArgs {
    /// The token to send. Defaults to DRT.
    #[arg(long, default_value = "drt")]
    pub token: SendToken,
    /// The destination Dytallix address.
    pub address: String,
    /// The token amount to send.
    pub amount: u128,
}

/// CLI token selector for send operations.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SendToken {
    /// Dytallix Governance Token.
    Dgt,
    /// Dytallix Reward Token.
    Drt,
}

impl From<SendToken> for Token {
    fn from(value: SendToken) -> Self {
        match value {
            SendToken::Dgt => Token::DGT,
            SendToken::Drt => Token::DRT,
        }
    }
}

/// Runs the `send` command.
pub async fn run(args: SendArgs) -> Result<()> {
    let destination = validate_destination_before_network(&args.address)?;

    let keystore = load_keystore()?;
    let sender = active_entry(&keystore)?.address.clone();
    let keypair = active_keypair(&keystore)?;
    let client = configured_client().await?;
    let account = client
        .get_account(&sender)
        .await
        .map_err(humanize_sdk_error)?;

    let token: Token = args.token.into();
    match token {
        Token::DGT if account.balance.dgt < args.amount => {
            return Err(anyhow!(
                "Insufficient balance for DGT. Required: {} DGT. Available: {} DGT.",
                format_number(args.amount),
                format_number(account.balance.dgt)
            ));
        }
        Token::DRT if account.balance.drt < args.amount => {
            return Err(anyhow!(
                "Insufficient balance for DRT. Required: {} DRT. Available: {} DRT.",
                format_number(args.amount),
                format_number(account.balance.drt)
            ));
        }
        _ => {}
    }

    let tx = TransactionBuilder::new()
        .from(sender)
        .to(destination)
        .amount(args.amount, token)
        .nonce(account.nonce)
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let fee = tx.estimate_fee(&client).await.map_err(humanize_sdk_error)?;

    let required_fee_micro = fee.total_cost_drt;
    let available_fee_micro = account.balance.dgt.saturating_mul(MICROS_PER_TOKEN);
    if available_fee_micro < required_fee_micro {
        return Err(anyhow!(
			"Insufficient DGT for gas fees. Required: {} DGT. Available: {} DGT. Run dytallix faucet to get more.",
			format_micro_amount(required_fee_micro),
			format_number(account.balance.dgt)
		));
    }

    output::fee_breakdown(&fee);
    let signed = tx.sign(&keypair).map_err(humanize_sdk_error)?;
    let receipt = client
        .submit_transaction(&signed)
        .await
        .map_err(humanize_sdk_error)?;
    let tx_hash = receipt.hash;
    output::tx_hash(&tx_hash);
    output::success("Transaction submitted", None);

    for attempt in 0..SEND_CONFIRMATION_POLL_COUNT {
        match client.get_transaction(&tx_hash).await {
            Ok(indexed) if !matches!(indexed.status, dytallix_sdk::TransactionStatus::Pending) => {
                println!("Status: {:?}", indexed.status);
                output::success("Transaction confirmed", None);
                return Ok(());
            }
            _ if attempt + 1 < SEND_CONFIRMATION_POLL_COUNT => {
                sleep(SEND_CONFIRMATION_POLL_INTERVAL).await;
            }
            _ => {}
        }
    }

    output::warning(
        "Transaction submitted but is still pending or indexing. Re-run dytallix balance in a moment or inspect the printed transaction hash.",
    );
    Ok(())
}

fn validate_destination_before_network(raw: &str) -> Result<dytallix_core::address::DAddr> {
    validate_address(raw)
}

#[cfg(test)]
mod tests {
    use super::validate_destination_before_network;

    #[test]
    fn send_validates_address_before_network_call() {
        let error = validate_destination_before_network("not-a-dytallix-address")
            .unwrap_err()
            .to_string();
        assert!(error.contains("checksum failed"));
    }
}
