//! Contract command implementation.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use dytallix_sdk::transaction::{estimate_default_gas_limits, Message, Transaction};
use serde_json::{json, Value};

use crate::commands::{
    active_entry, active_keypair, bytes_to_hex, configured_client, display_path,
    format_micro_amount, format_number, hex_to_bytes, humanize_sdk_error, load_keystore,
    raw_get_json, raw_post_json, read_bytes,
};
use crate::output;

const DEPLOY_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(15);
const DEPLOY_CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_millis(750);
const CALL_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_millis(750);
const CONTRACT_DEPLOY_GAS_LIMIT: u64 = 1_000_000;
const CONTRACT_CALL_GAS_LIMIT: u64 = 1_000_000;
const MICROS_PER_TOKEN: u128 = 1_000_000;

/// Arguments for the `contract` command.
#[derive(Debug, Clone, Args)]
pub struct ContractArgs {
    /// Contract subcommand.
    #[command(subcommand)]
    pub command: ContractCommand,
}

/// Contract subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ContractCommand {
    /// Deploy a WASM contract.
    Deploy { wasm_file: PathBuf },
    /// Call a contract method by submitting a transaction.
    Call {
        address: String,
        method: String,
        args: Vec<String>,
    },
    /// Query a contract method without submitting a transaction.
    Query {
        address: String,
        method: String,
        args: Vec<String>,
    },
    /// Show contract metadata.
    Info { address: String },
    /// Show contract events.
    Events { address: String },
}

/// Runs the `contract` command.
pub async fn run(args: ContractArgs) -> Result<()> {
    match args.command {
        ContractCommand::Deploy { wasm_file } => deploy(wasm_file).await,
        ContractCommand::Call {
            address,
            method,
            args,
        } => call(address, method, args).await,
        ContractCommand::Query {
            address,
            method,
            args,
        } => query(address, method, args).await,
        ContractCommand::Info { address } => info(address).await,
        ContractCommand::Events { address } => events(address).await,
    }
}

async fn deploy(wasm_file: PathBuf) -> Result<()> {
    let wasm = validated_wasm_bytes(&wasm_file)?;
    // The live node's `POST /contracts/deploy` expects the raw contract `code`
    // (hex) with an optional `deployer` and `gas_limit`. It does not verify a
    // signed transaction and does not charge a DRT fee. Posting a signed
    // `ContractDeploy` message instead makes the node reject the request with
    // `missing code`, so send the fields the node actually reads.
    let keystore = load_keystore()?;
    let deployer = active_entry(&keystore)?.address.to_string();
    let value = raw_post_json(
        "/contracts/deploy",
        &json!({
            "code": bytes_to_hex(&wasm),
            "deployer": deployer,
            "gas_limit": CONTRACT_DEPLOY_GAS_LIMIT,
        }),
    )
    .await?;
    let tx_hash = value
        .get("tx_hash")
        .and_then(|raw| raw.as_str())
        .map(str::to_owned);
    let address = value
        .get("address")
        .and_then(|raw| raw.as_str())
        .map(str::to_owned);

    if let Some(tx_hash) = tx_hash.as_deref() {
        output::tx_hash(tx_hash);
    }
    if let Some(address) = address.as_deref() {
        println!("Contract address: {address}");
    }

    let mut services = RealDeployConfirmationServices;
    match wait_for_deploy_confirmation(
        &mut services,
        tx_hash.as_deref(),
        address.as_deref(),
        DEPLOY_CONFIRMATION_TIMEOUT,
        DEPLOY_CONFIRMATION_POLL_INTERVAL,
    )
    .await?
    {
        Some(confirmation) => {
            output::success("Contract deployment confirmed", Some(confirmation.elapsed));
            match confirmation.via {
                DeployConfirmationVia::Transaction => {
                    if let Some(tx_hash) = tx_hash.as_deref() {
                        println!("Indexed via: /tx/{tx_hash}");
                    }
                    if address.is_some() {
                        print_canonical_contract_verification(address.as_deref());
                    }
                }
                DeployConfirmationVia::Contract => {
                    if let Some(address) = address.as_deref() {
                        println!("Indexed via: /api/contracts/{address}");
                    }
                    output::warning(
                        "Contract metadata is available now. The transaction receipt route may still be indexing.",
                    );
                    print_canonical_contract_verification(address.as_deref());
                }
            }
        }
        None => {
            output::warning(&format!(
                "Contract deployment submitted but was not indexed within {:.1}s.",
                DEPLOY_CONFIRMATION_TIMEOUT.as_secs_f64()
            ));
            print_canonical_contract_verification(address.as_deref());
        }
    }
    Ok(())
}

async fn call(address: String, method: String, args: Vec<String>) -> Result<()> {
    let contract = validate_contract_address(&address)?;
    let method = method.trim();
    let signed = sign_contract_transaction(Message::ContractCall {
        from: String::new(),
        address: contract.clone(),
        method: method.to_owned(),
        args: Some(encode_contract_args(&args)),
        gas_limit: CONTRACT_CALL_GAS_LIMIT,
    })
    .await?;
    let value = raw_post_json("/contracts/call", &json!({ "signed_tx": signed })).await?;
    let tx_hash = value
        .get("tx_hash")
        .and_then(|raw| raw.as_str())
        .map(str::to_owned);
    if let Some(tx_hash) = tx_hash.as_deref() {
        output::tx_hash(tx_hash);
    }

    if let Some(tx_hash) = tx_hash.as_deref() {
        let mut services = RealDeployConfirmationServices;
        if wait_for_transaction_confirmation(
            &mut services,
            tx_hash,
            CALL_CONFIRMATION_TIMEOUT,
            CALL_CONFIRMATION_POLL_INTERVAL,
        )
        .await?
        {
            output::success("Contract call confirmed", None);
            println!("Indexed via: /tx/{tx_hash}");
        } else {
            output::warning(&format!(
                "Contract call submitted but was not indexed within {:.1}s.",
                CALL_CONFIRMATION_TIMEOUT.as_secs_f64()
            ));
        }
    }

    output::section("Contract call");
    println!("{}", serde_json::to_string_pretty(&value)?);
    output::success("Contract call submitted", None);
    Ok(())
}

async fn query(address: String, method: String, args: Vec<String>) -> Result<()> {
    let contract = validate_contract_address(&address)?;
    let mut path = format!("/api/contracts/{contract}/query/{method}");
    let encoded_args = encode_contract_args(&args);
    if !encoded_args.is_empty() {
        path.push_str(&format!("?args={encoded_args}"));
    }
    let value = raw_get_json(&path).await?;
    output::section("Contract query");
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn info(address: String) -> Result<()> {
    let contract = validate_contract_address(&address)?;
    let contract_info = raw_get_json(&format!("/api/contracts/{contract}")).await?;
    output::section("Contract info");
    println!("{}", serde_json::to_string_pretty(&contract_info)?);
    Ok(())
}

async fn events(address: String) -> Result<()> {
    let contract = validate_contract_address(&address)?;
    let value = raw_get_json(&format!("/api/contracts/{contract}/events")).await?;
    output::section("Contract events");
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn validated_wasm_bytes(path: &Path) -> Result<Vec<u8>> {
    if !path.exists() {
        return Err(anyhow!(
            "Contract file not found at {}. Check the path and try again.",
            display_path(path)
        ));
    }

    let bytes = read_bytes(path)?;
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        return Err(anyhow!(
            "File at {} is not a valid WASM binary.",
            display_path(path)
        ));
    }
    Ok(bytes)
}

fn validate_contract_address(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let hex = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if hex.len() != 40 {
        return Err(anyhow!(
            "Invalid contract address `{trimmed}`. Expected a 0x-prefixed 20-byte hex address."
        ));
    }
    hex_to_bytes(hex)?;
    Ok(format!("0x{hex}"))
}

fn encode_contract_args(args: &[String]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        bytes_to_hex(args.join(",").as_bytes())
    }
}

fn print_canonical_contract_verification(address: Option<&str>) {
    if let Some(address) = address {
        println!("Verify with: dytallix contract info {address}");
    }
}

async fn sign_contract_transaction(
    mut message: Message,
) -> Result<dytallix_sdk::transaction::SignedTransaction> {
    let keystore = load_keystore()?;
    let active = active_entry(&keystore)?;
    let keypair = active_keypair(&keystore)?;

    match &mut message {
        Message::ContractDeploy { from, .. } | Message::ContractCall { from, .. } => {
            *from = active.address.to_string();
        }
        _ => return Err(anyhow!("unsupported contract message type")),
    }

    let client = configured_client().await?;
    let account = client
        .get_account(&active.address)
        .await
        .map_err(humanize_sdk_error)?;
    let chain_status = client
        .get_chain_status()
        .await
        .map_err(humanize_sdk_error)?;
    let (c_gas_limit, b_gas_limit) = estimate_default_gas_limits(std::slice::from_ref(&message));

    let tx = Transaction {
        chain_id: chain_status.finalized_checkpoint,
        nonce: account.nonce,
        msgs: vec![message],
        fee: 0,
        memo: String::new(),
        c_gas_limit,
        b_gas_limit,
    };
    let fee = tx.estimate_fee(&client).await.map_err(humanize_sdk_error)?;
    let required_fee_micro = fee.total_cost_drt;
    let available_fee_micro = account.balance.drt.saturating_mul(MICROS_PER_TOKEN);
    if available_fee_micro < required_fee_micro {
        return Err(anyhow!(
            "Insufficient DRT for gas fees. Required: {} DRT. Available: {} DRT. Run dytallix faucet to get more.",
            format_micro_amount(required_fee_micro),
            format_number(account.balance.drt)
        ));
    }

    output::fee_breakdown(&fee);
    tx.with_fee_micro(required_fee_micro)
        .sign(&keypair)
        .map_err(humanize_sdk_error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeployConfirmationVia {
    Transaction,
    Contract,
}

#[derive(Debug)]
struct DeployConfirmation {
    via: DeployConfirmationVia,
    elapsed: Duration,
}

trait DeployConfirmationServices {
    async fn get_json(&mut self, path: &str) -> Result<Value>;
    async fn wait(&mut self, duration: Duration);
}

struct RealDeployConfirmationServices;

impl DeployConfirmationServices for RealDeployConfirmationServices {
    async fn get_json(&mut self, path: &str) -> Result<Value> {
        raw_get_json(path).await
    }

    async fn wait(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

async fn wait_for_deploy_confirmation<S>(
    services: &mut S,
    tx_hash: Option<&str>,
    address: Option<&str>,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<Option<DeployConfirmation>>
where
    S: DeployConfirmationServices,
{
    if tx_hash.is_none() && address.is_none() {
        return Ok(None);
    }

    let started = Instant::now();
    loop {
        if let Some(tx_hash) = tx_hash {
            if services.get_json(&format!("/tx/{tx_hash}")).await.is_ok() {
                return Ok(Some(DeployConfirmation {
                    via: DeployConfirmationVia::Transaction,
                    elapsed: started.elapsed(),
                }));
            }
        }

        if let Some(address) = address {
            if services
                .get_json(&format!("/api/contracts/{address}"))
                .await
                .is_ok()
            {
                return Ok(Some(DeployConfirmation {
                    via: DeployConfirmationVia::Contract,
                    elapsed: started.elapsed(),
                }));
            }
        }

        if started.elapsed() >= timeout {
            return Ok(None);
        }

        services.wait(poll_interval).await;
    }
}

async fn wait_for_transaction_confirmation<S>(
    services: &mut S,
    tx_hash: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<bool>
where
    S: DeployConfirmationServices,
{
    let started = Instant::now();
    loop {
        if services.get_json(&format!("/tx/{tx_hash}")).await.is_ok() {
            return Ok(true);
        }

        if started.elapsed() >= timeout {
            return Ok(false);
        }

        services.wait(poll_interval).await;
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
struct DeployConvergence {
    tx_visible: bool,
    contract_visible: bool,
    elapsed: Duration,
}

#[cfg(test)]
async fn wait_for_deploy_convergence<S>(
    services: &mut S,
    tx_hash: &str,
    address: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<DeployConvergence>
where
    S: DeployConfirmationServices,
{
    let started = Instant::now();
    let mut convergence = DeployConvergence::default();

    loop {
        if !convergence.tx_visible && services.get_json(&format!("/tx/{tx_hash}")).await.is_ok() {
            convergence.tx_visible = true;
        }
        if !convergence.contract_visible
            && services
                .get_json(&format!("/api/contracts/{address}"))
                .await
                .is_ok()
        {
            convergence.contract_visible = true;
        }
        convergence.elapsed = started.elapsed();

        if convergence.tx_visible && convergence.contract_visible {
            return Ok(convergence);
        }
        if convergence.elapsed >= timeout {
            return Ok(convergence);
        }

        services.wait(poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::{
        encode_contract_args, validate_contract_address, wait_for_deploy_confirmation,
        wait_for_deploy_convergence, wait_for_transaction_confirmation, DeployConfirmationServices,
        DeployConfirmationVia,
    };

    #[derive(Default)]
    struct MockDeployConfirmationServices {
        responses: HashMap<String, Vec<Result<Value, String>>>,
        waits: Vec<Duration>,
    }

    impl MockDeployConfirmationServices {
        fn push_ok(&mut self, path: &str, value: Value) {
            self.responses
                .entry(path.to_owned())
                .or_default()
                .push(Ok(value));
        }

        fn push_err(&mut self, path: &str, message: &str) {
            self.responses
                .entry(path.to_owned())
                .or_default()
                .push(Err(message.to_owned()));
        }
    }

    impl DeployConfirmationServices for MockDeployConfirmationServices {
        async fn get_json(&mut self, path: &str) -> anyhow::Result<Value> {
            let queue = self
                .responses
                .get_mut(path)
                .unwrap_or_else(|| panic!("unexpected path {path}"));
            let response = queue.remove(0);
            match response {
                Ok(value) => Ok(value),
                Err(message) => Err(anyhow::anyhow!(message)),
            }
        }

        async fn wait(&mut self, duration: Duration) {
            self.waits.push(duration);
        }
    }

    #[test]
    fn contract_address_validation_accepts_prefixed_hex() {
        let address =
            validate_contract_address("0x9a9671441249ee2c364f9b4bc8049e61b082449a").unwrap();
        assert_eq!(address, "0x9a9671441249ee2c364f9b4bc8049e61b082449a");
    }

    #[test]
    fn contract_args_are_hex_encoded() {
        assert_eq!(
            encode_contract_args(&["hello".to_owned(), "7".to_owned()]),
            "68656c6c6f2c37"
        );
    }

    #[tokio::test]
    async fn deploy_confirmation_uses_contract_metadata_when_receipt_route_lags() {
        let mut services = MockDeployConfirmationServices::default();
        services.push_err("/tx/0xabc", "not ready");
        services.push_ok(
            "/api/contracts/0xdef",
            json!({ "address": "0xdef", "tx_hash": "0xabc" }),
        );

        let confirmation = wait_for_deploy_confirmation(
            &mut services,
            Some("0xabc"),
            Some("0xdef"),
            Duration::from_secs(5),
            Duration::from_millis(250),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(confirmation.via, DeployConfirmationVia::Contract);
        assert!(services.waits.is_empty());
    }

    #[tokio::test]
    async fn deploy_visibility_surfaces_converge_within_expected_window() {
        let mut services = MockDeployConfirmationServices::default();
        services.push_err("/tx/0xabc", "pending");
        services.push_err("/tx/0xabc", "pending");
        services.push_ok(
            "/tx/0xabc",
            json!({ "tx_hash": "0xabc", "status": "Success" }),
        );

        services.push_err("/api/contracts/0xdef", "missing");
        services.push_ok("/api/contracts/0xdef", json!({ "address": "0xdef" }));

        let convergence = wait_for_deploy_convergence(
            &mut services,
            "0xabc",
            "0xdef",
            Duration::from_secs(3),
            Duration::from_millis(250),
        )
        .await
        .unwrap();

        assert!(convergence.tx_visible);
        assert!(convergence.contract_visible);
        assert!(convergence.elapsed <= Duration::from_secs(3));
        assert_eq!(services.waits.len(), 2);
    }

    #[tokio::test]
    async fn transaction_confirmation_returns_true_when_receipt_appears() {
        let mut services = MockDeployConfirmationServices::default();
        services.push_err("/tx/0xabc", "pending");
        services.push_ok("/tx/0xabc", json!({ "tx_hash": "0xabc" }));

        let confirmed = wait_for_transaction_confirmation(
            &mut services,
            "0xabc",
            Duration::from_secs(2),
            Duration::from_millis(100),
        )
        .await
        .unwrap();

        assert!(confirmed);
        assert_eq!(services.waits, vec![Duration::from_millis(100)]);
    }

    #[tokio::test]
    async fn transaction_confirmation_returns_false_on_timeout() {
        let mut services = MockDeployConfirmationServices::default();
        services.push_err("/tx/0xdef", "pending");

        let confirmed = wait_for_transaction_confirmation(
            &mut services,
            "0xdef",
            Duration::from_millis(0),
            Duration::from_millis(100),
        )
        .await
        .unwrap();

        assert!(!confirmed);
        assert!(services.waits.is_empty());
    }
}
