//! Legacy testnet REST helpers (`legacy-network` feature): the public
//! gateway, faucet and node REST API. The consensus chain uses none of them.
use anyhow::{anyhow, Context, Result};
use dytallix_core::address::DAddr;
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::error::SdkError;
use dytallix_sdk::faucet::FaucetClient;
use dytallix_sdk::FaucetStatus;
use serde_json::{json, Value};

use super::{humanize_sdk_error, load_config, CliConfig, NetworkProfile};

pub(crate) const TESTNET_ENDPOINT: &str = "https://dytallix.com";
pub(crate) const LOCAL_ENDPOINT: &str = "http://localhost:3030";
pub(crate) const TESTNET_FAUCET: &str = "https://dytallix.com/api/faucet";
pub(crate) const LOCAL_FAUCET: &str = "http://localhost:3030/dev/faucet";
pub(crate) const DISCORD_LINK: &str = "https://discord.gg/eyVvu5kmPG";
pub(crate) const EXPLORER_LINK: &str = "https://dytallix.com/build/blockchain";
pub(crate) const ENDPOINT_OVERRIDE_KEY: &str = "endpoint";
pub(crate) const LOCAL_FAUCET_DEFAULT_UDGT: u64 = 10_000_000;
pub(crate) const LOCAL_FAUCET_DEFAULT_UDRT: u64 = 100_000_000;

pub(crate) async fn configured_client() -> Result<DytallixClient> {
    let config = load_config()?;
    let endpoint = configured_network_endpoint(&config)?;
    DytallixClient::new(&endpoint)
        .await
        .map_err(humanize_sdk_error)
}

pub(crate) fn configured_faucet() -> Result<FaucetClient> {
    let config = load_config()?;
    let endpoint = match config.network {
        NetworkProfile::Testnet => TESTNET_FAUCET,
        NetworkProfile::Local => LOCAL_FAUCET,
        NetworkProfile::Mainnet => {
            return Err(anyhow!(
				"Faucet is not available on mainnet. Switch to testnet with `dytallix config network testnet`."
			));
        }
    };

    Ok(FaucetClient::new(endpoint))
}

pub(crate) fn faucet_endpoint(profile: NetworkProfile) -> Result<&'static str> {
    match profile {
        NetworkProfile::Testnet => Ok(TESTNET_FAUCET),
        NetworkProfile::Local => Ok(LOCAL_FAUCET),
        NetworkProfile::Mainnet => Err(anyhow!(
            "Faucet is not available on mainnet. Switch to testnet with `dytallix config network testnet`."
        )),
    }
}

pub(crate) fn network_endpoint(profile: NetworkProfile) -> Result<&'static str> {
    match profile {
        NetworkProfile::Testnet => Ok(TESTNET_ENDPOINT),
        NetworkProfile::Local => Ok(LOCAL_ENDPOINT),
        NetworkProfile::Mainnet => Err(anyhow!(
            "Mainnet is not publicly available yet. Switch to testnet with `dytallix config network testnet`."
        )),
    }
}

pub(crate) fn configured_network_endpoint(config: &CliConfig) -> Result<String> {
    if let Ok(endpoint) = std::env::var("DYTALLIX_ENDPOINT") {
        return normalize_endpoint_override(&endpoint);
    }

    if let Some(endpoint) = config.values.get(ENDPOINT_OVERRIDE_KEY) {
        return normalize_endpoint_override(endpoint);
    }

    Ok(network_endpoint(config.network)?.to_owned())
}

pub(crate) fn public_website_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint.trim_end_matches('/'),
        TESTNET_ENDPOINT | "https://www.dytallix.com"
    )
}

pub(crate) async fn ensure_public_gateway_write_allowed(
    feature: &str,
    feature_state_key: &str,
    public_read_hint: &str,
) -> Result<()> {
    let config = load_config()?;
    let endpoint = configured_network_endpoint(&config)?;
    if public_website_endpoint(&endpoint) {
        let client = DytallixClient::new(&endpoint)
            .await
            .map_err(humanize_sdk_error)?;
        let feature_state = client
            .public_feature_state(feature_state_key)
            .await
            .map_err(humanize_sdk_error)?;
        if public_write_state_disallows_gateway(feature_state.as_deref()) {
            return Err(anyhow!(public_gateway_write_unavailable_message(
                feature,
                public_read_hint,
            )));
        }
    }
    Ok(())
}

pub(crate) fn public_write_state_disallows_gateway(feature_state: Option<&str>) -> bool {
    matches!(
        feature_state,
        None | Some("hidden" | "disabled" | "operator-preview")
    )
}

pub(crate) fn public_gateway_write_unavailable_message(
    feature: &str,
    public_read_hint: &str,
) -> String {
    format!(
        "Public {feature} writes are disabled on the default public website gateway while this protocol path is still alpha-incomplete. Use `{public_read_hint}` for read-only checks, inspect `/api/capabilities` on a compatible node for the active contract, or point the CLI at a direct node or local endpoint with `dytallix config set endpoint http://localhost:3030` or `DYTALLIX_ENDPOINT`."
    )
}

pub(crate) fn normalize_endpoint_override(raw: &str) -> Result<String> {
    let endpoint = raw.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(anyhow!(
            "Configured endpoint override is empty. Set a full http:// or https:// base URL."
        ));
    }
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err(anyhow!(
            "Configured endpoint override `{endpoint}` must start with http:// or https://."
        ));
    }
    Ok(endpoint.to_string())
}

pub(crate) fn validate_address(raw: &str) -> Result<DAddr> {
    DAddr::from_str(raw)
        .map_err(|_| anyhow!("Invalid address: Bech32m checksum failed — check for typos."))
}

pub(crate) fn short_address(address: &DAddr) -> String {
    let prefix = address.as_str().chars().take(16).collect::<String>();
    format!("{prefix}...")
}

pub(crate) async fn raw_get_json(path: &str) -> Result<Value> {
    let config = load_config()?;
    let endpoint = configured_network_endpoint(&config)?;
    raw_get_json_at(&endpoint, path).await
}

pub(crate) async fn raw_get_json_at(endpoint: &str, path: &str) -> Result<Value> {
    let client = DytallixClient::new(endpoint)
        .await
        .map_err(humanize_sdk_error)?;
    let effective_path = client
        .resolve_read_path(path)
        .await
        .map_err(humanize_sdk_error)?;
    let url = format!("{endpoint}{effective_path}");
    let response = reqwest::get(&url)
        .await
        .map_err(|_| anyhow!("Cannot reach {url}. Check your network connection."))?;
    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|err| anyhow!("Failed to decode response from {url}: {err}"))
    } else {
        let status = response.status();
        let reason = response.text().await.unwrap_or_default();
        if let Some(message) = public_gateway_contract_read_hint(endpoint, path, status) {
            return Err(anyhow!(message));
        }
        Err(anyhow!(
            "Request to {url} failed with status {status}. {reason}"
        ))
    }
}

pub(crate) async fn raw_post_json(path: &str, payload: &Value) -> Result<Value> {
    let config = load_config()?;
    let endpoint = configured_network_endpoint(&config)?;
    raw_post_json_at(&endpoint, path, payload).await
}

pub(crate) async fn raw_post_json_at(endpoint: &str, path: &str, payload: &Value) -> Result<Value> {
    let url = format!("{endpoint}{path}");
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(payload)
        .send()
        .await
        .map_err(|_| anyhow!("Cannot reach {url}. Check your network connection."))?;
    if response.status().is_success() {
        response
            .json()
            .await
            .map_err(|err| anyhow!("Failed to decode response from {url}: {err}"))
    } else {
        let status = response.status();
        let reason = response.text().await.unwrap_or_default();
        if let Some(message) = public_gateway_contract_write_hint(endpoint, path, status) {
            return Err(anyhow!(message));
        }
        Err(anyhow!(
            "Request to {url} failed with status {status}. {reason}"
        ))
    }
}

pub(crate) fn public_gateway_contract_write_hint(
    endpoint: &str,
    path: &str,
    status: reqwest::StatusCode,
) -> Option<String> {
    let endpoint = endpoint.trim_end_matches('/');
    let is_public_website = public_website_endpoint(endpoint);
    let is_contract_write = matches!(path, "/contracts/deploy" | "/contracts/call");
    let is_gateway_rejection = matches!(
        status,
        reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
    );

    if is_public_website && is_contract_write && is_gateway_rejection {
        Some(format!(
            "The public website endpoint at {endpoint} returned {status} for `{path}`. If this persists, use `dytallix config set endpoint http://localhost:3030` for a local node or set `DYTALLIX_ENDPOINT` to a direct node that serves contract write routes."
        ))
    } else {
        None
    }
}

pub(crate) fn public_gateway_contract_read_hint(
    endpoint: &str,
    path: &str,
    status: reqwest::StatusCode,
) -> Option<String> {
    let endpoint = endpoint.trim_end_matches('/');
    let is_public_website = public_website_endpoint(endpoint);
    let is_contract_read = path.starts_with("/api/contracts/");
    let is_gateway_rejection = matches!(
        status,
        reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
    );

    if is_public_website && is_contract_read && is_gateway_rejection {
        Some(format!(
            "The public website gateway at {endpoint} is not currently serving `{path}`. Use `dytallix config set endpoint http://localhost:3030` for a local node or set `DYTALLIX_ENDPOINT` to a direct node."
        ))
    } else {
        None
    }
}

pub(crate) async fn faucet_request(address: &DAddr, token_type: &str) -> Result<()> {
    let config = load_config()?;
    if config.network == NetworkProfile::Local {
        let endpoint = configured_network_endpoint(&config)?;
        return local_faucet_request(&endpoint, address, token_type).await;
    }

    let faucet = configured_faucet()?;
    match token_type.to_ascii_lowercase().as_str() {
        "both" => faucet.fund(address).await.map(|_| ()),
        "dgt" => faucet.fund_dgt(address).await.map(|_| ()),
        "drt" => faucet.fund_drt(address).await.map(|_| ()),
        other => Err(SdkError::FaucetUnavailable {
            endpoint: faucet_endpoint(load_config()?.network)?.to_owned(),
            reason: format!("unsupported faucet token selection: {other}"),
        }),
    }
    .map_err(humanize_sdk_error)
}

pub(crate) async fn faucet_status(address: &DAddr) -> Result<FaucetStatus> {
    let config = load_config()?;
    if config.network == NetworkProfile::Local {
        let endpoint = configured_network_endpoint(&config)?;
        return local_faucet_status(&endpoint, address).await;
    }

    let faucet = configured_faucet()?;
    match faucet.status(address).await {
        Ok(status) => Ok(status),
        Err(SdkError::FaucetRateLimited {
            retry_after_seconds,
        }) => Ok(FaucetStatus {
            can_request: false,
            retry_after_seconds: Some(retry_after_seconds),
        }),
        Err(error) => Err(humanize_sdk_error(error)),
    }
}

pub(crate) async fn local_faucet_request(
    endpoint: &str,
    address: &DAddr,
    token_type: &str,
) -> Result<()> {
    let (udgt, udrt) = match token_type.to_ascii_lowercase().as_str() {
        "both" => (LOCAL_FAUCET_DEFAULT_UDGT, LOCAL_FAUCET_DEFAULT_UDRT),
        "dgt" => (LOCAL_FAUCET_DEFAULT_UDGT, 0),
        "drt" => (0, LOCAL_FAUCET_DEFAULT_UDRT),
        other => {
            return Err(anyhow!(
                "Unsupported faucet token selection `{other}`. Use dgt, drt, or both."
            ));
        }
    };

    let response = raw_post_json_at(
        endpoint,
        "/dev/faucet",
        &json!({
            "address": address,
            "udgt": udgt,
            "udrt": udrt,
        }),
    )
    .await?;

    let success = response
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if success {
        Ok(())
    } else {
        Err(anyhow!(
            "Local faucet rejected the request. Ensure the local node is running at {endpoint} and exposes POST /dev/faucet."
        ))
    }
}

pub(crate) async fn local_faucet_status(endpoint: &str, _address: &DAddr) -> Result<FaucetStatus> {
    raw_get_json_at(endpoint, "/status")
        .await
        .map_err(|_| {
            anyhow!(
                "Cannot reach local node status at {endpoint}/status. Start it with `./start-local.sh` (from the dytallix-sdk repo root) or set `DYTALLIX_ENDPOINT` to a reachable direct node."
            )
        })?;

    Ok(FaucetStatus {
        can_request: true,
        retry_after_seconds: None,
    })
}

pub(crate) async fn faucet_balance(address: &DAddr) -> Result<dytallix_sdk::Balance> {
    configured_client()
        .await?
        .get_balance(address)
        .await
        .map_err(humanize_sdk_error)
}

pub(crate) fn faucet_balance_timeout(address: &DAddr) -> anyhow::Error {
    anyhow!(
		"Faucet request submitted but balance not confirmed after 45 seconds. Check the explorer at {EXPLORER_LINK} for address {address}. Join Discord at {DISCORD_LINK} if the problem persists."
	)
}

pub(crate) fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let command = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let command = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let command = ("cmd", vec!["/C", "start", url]);

    let status = std::process::Command::new(command.0)
        .args(command.1)
        .status()
        .with_context(|| format!("Failed to open {url}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to open {url}. Open it manually in your browser."
        ))
    }
}

#[cfg(test)]
mod tests {
    use dytallix_sdk::error::SdkError;

    use super::{
        faucet_balance_timeout, faucet_endpoint, network_endpoint, normalize_endpoint_override,
        public_gateway_write_unavailable_message, public_website_endpoint,
        public_write_state_disallows_gateway, LOCAL_ENDPOINT, TESTNET_ENDPOINT, TESTNET_FAUCET,
    };
    use crate::commands::{humanize_sdk_error, keystore_not_found_message, NetworkProfile};
    use dytallix_core::address::DAddr;
    use dytallix_core::keypair::DytallixKeypair;

    #[test]
    fn error_messages_are_correct() {
        let rate_limited = humanize_sdk_error(SdkError::FaucetRateLimited {
            retry_after_seconds: 17,
        })
        .to_string();
        assert!(rate_limited.contains("Try again in 17 seconds"));

        let node_unavailable = humanize_sdk_error(SdkError::NodeUnavailable {
            endpoint: "https://dytallix.com".to_owned(),
            reason: "offline".to_owned(),
        })
        .to_string();
        assert!(node_unavailable.contains("Check your network connection"));

        let tx_api_unavailable = humanize_sdk_error(SdkError::NodeUnavailable {
            endpoint: "https://dytallix.com/api/blockchain/submit".to_owned(),
            reason: "<html><h1>405 Not Allowed</h1></html>".to_owned(),
        })
        .to_string();
        assert!(tx_api_unavailable.contains("transaction API is not available"));

        assert!(keystore_not_found_message().contains("Run dytallix init"));

        let address = DAddr::from_public_key(DytallixKeypair::generate().public_key()).unwrap();
        let timeout = faucet_balance_timeout(&address).to_string();
        assert!(timeout.contains("discord.gg/eyVvu5kmPG"));
    }

    #[test]
    fn network_profiles_use_public_surface_defaults() {
        assert_eq!(
            network_endpoint(NetworkProfile::Testnet).unwrap(),
            TESTNET_ENDPOINT
        );
        assert_eq!(
            network_endpoint(NetworkProfile::Local).unwrap(),
            LOCAL_ENDPOINT
        );
        assert_eq!(
            faucet_endpoint(NetworkProfile::Testnet).unwrap(),
            TESTNET_FAUCET
        );
    }

    #[test]
    fn endpoint_override_is_normalized() {
        assert_eq!(
            normalize_endpoint_override("https://rpc.example.test/").unwrap(),
            "https://rpc.example.test"
        );
        assert!(normalize_endpoint_override("rpc.example.test").is_err());
    }

    #[test]
    fn public_website_endpoint_detection_matches_gateway_hosts() {
        assert!(public_website_endpoint(TESTNET_ENDPOINT));
        assert!(public_website_endpoint("https://www.dytallix.com/"));
        assert!(!public_website_endpoint(LOCAL_ENDPOINT));
    }

    #[test]
    fn public_gateway_write_guard_is_descriptive() {
        let error = public_gateway_write_unavailable_message("staking", "dytallix stake status");
        assert!(error.contains("Public staking writes are disabled"));
        assert!(error.contains("dytallix stake status"));
        assert!(error.contains("/api/capabilities"));
    }

    #[test]
    fn hidden_and_unknown_feature_states_block_public_writes() {
        assert!(public_write_state_disallows_gateway(Some("hidden")));
        assert!(public_write_state_disallows_gateway(Some(
            "operator-preview"
        )));
        assert!(public_write_state_disallows_gateway(None));
        assert!(!public_write_state_disallows_gateway(Some(
            "supported-alpha"
        )));
    }
}
