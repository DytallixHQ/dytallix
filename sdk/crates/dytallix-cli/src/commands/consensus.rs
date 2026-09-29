//! Shared one-step flow for consensus-chain writes (clients v1, K-c): read
//! the signing context from the configured node, refuse it unless the node
//! reports the pinned chain, prepare and sign locally, submit, and wait for
//! the committed result (P01, 27 September 2026, decisions 4 and 5). The gas
//! limit and fee cap are always explicit.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, ensure, Context, Result};
use clap::{Args, ValueEnum};
use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
use dytallix_sdk::ordinary_client::CometClient;
use dytallix_sdk::ordinary_v2::{
    self as ordinary, AccountAddress, Action, AddressNetwork, ChainPin, FeeQuote, KeyIdentity,
    KeypairSigner,
};
use dytallix_sdk::ordinary_v3 as governance;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::bytes_to_hex;
use super::ordinary::{endpoint as parse_endpoint, load_signing_key, MAX_RESPONSE_BYTES};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_sdk::ordinary_client::{Endpoint, EndpointPin};

/// Commitment is polled once a second for `--wait-seconds`.
const WAIT_INTERVAL: Duration = Duration::from_secs(1);

/// Version of `chain.json` (interfaces v1); a file without one is version 1.
/// Version 2 (E04 gap 19) adds the endpoint key; version 1 files still load,
/// with a loopback endpoint only.
pub(crate) const CHAIN_CONFIG_VERSION: u32 = 2;

/// The pinned chain and its node, from `dytallix config pin-chain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChainConfig {
    #[serde(default = "super::first_version")]
    pub(crate) version: u32,
    /// A loopback `http://IP:PORT`, or a channel endpoint's `host:port`.
    pub(crate) endpoint: String,
    /// The channel endpoint's full ML-DSA-65 key, from its pin file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint_key_base64: Option<String>,
    pub(crate) network: Network,
    pub(crate) chain_id: String,
    /// Lowercase hexadecimal SHA-256 of the exact genesis file.
    pub(crate) genesis_digest: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Network {
    Mainnet,
    Testnet,
    Development,
}
impl From<Network> for AddressNetwork {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => AddressNetwork::Mainnet,
            Network::Testnet => AddressNetwork::Testnet,
            Network::Development => AddressNetwork::Development,
        }
    }
}
impl ChainConfig {
    /// A new pin from `pin-chain`: the endpoint is a loopback URL or a
    /// channel endpoint's pin file, which must name the same chain.
    pub(crate) fn new(
        endpoint: &str,
        network: Network,
        chain_id: String,
        genesis_digest: String,
    ) -> Result<Self> {
        let (endpoint, endpoint_key_base64) = match parse_endpoint(endpoint)? {
            Endpoint::Loopback(address) => (format!("http://{address}"), None),
            Endpoint::Channel(pin) => {
                ensure!(
                    pin.network == chain_id,
                    "the endpoint pin is for chain {}, not {chain_id}",
                    pin.network
                );
                (pin.address, Some(STANDARD.encode(&pin.public_key)))
            }
        };
        let config = ChainConfig {
            version: CHAIN_CONFIG_VERSION,
            endpoint,
            endpoint_key_base64,
            network,
            chain_id,
            genesis_digest,
        };
        config.pin()?;
        Ok(config)
    }
    /// The pinned endpoint. A loopback URL needs no key; a remote endpoint
    /// needs its key.
    pub(crate) fn endpoint(&self) -> Result<Endpoint> {
        let Some(encoded) = &self.endpoint_key_base64 else {
            return Endpoint::loopback(&self.endpoint).map_err(|e| {
                anyhow!("{e}; pin the chain again with the endpoint's pin file (`dytallix config pin-chain --endpoint PIN_FILE`)")
            });
        };
        let key = STANDARD
            .decode(encoded)
            .ok()
            .filter(|key| STANDARD.encode(key) == *encoded)
            .context("the pinned endpoint key is not canonical base64")?;
        let pin = EndpointPin::new(&self.chain_id, &self.endpoint, &key)
            .map_err(|_| anyhow!("the pinned endpoint is not a valid host:port and key"))?;
        Ok(Endpoint::Channel(pin))
    }
    /// A client for the pinned endpoint, or for an `--endpoint` override. A
    /// channel endpoint must be for the pinned chain.
    pub(crate) fn client(&self, endpoint: Option<&str>) -> Result<CometClient> {
        let endpoint = match endpoint {
            Some(value) => parse_endpoint(value)?,
            None => self.endpoint()?,
        };
        if let Endpoint::Channel(pin) = &endpoint {
            ensure!(
                pin.network == self.chain_id,
                "the endpoint pin is for chain {}, not the pinned chain {}",
                pin.network,
                self.chain_id
            );
        }
        Ok(CometClient::with_endpoint(endpoint, MAX_RESPONSE_BYTES)?)
    }
    pub(crate) fn path() -> PathBuf {
        super::cli_dir().join("chain.json")
    }
    pub(crate) fn load() -> Result<Self> {
        let path = Self::path();
        let raw = fs::read(&path).map_err(|_| {
            anyhow!("no pinned chain; run `dytallix config pin-chain` with values from a source you trust")
        })?;
        let config: Self = serde_json::from_slice(&raw)
            .with_context(|| format!("invalid pinned chain file {}", path.display()))?;
        config.pin()?;
        Ok(config)
    }
    pub(crate) fn save(&self) -> Result<()> {
        self.pin()?;
        super::ensure_cli_dir()?;
        fs::write(Self::path(), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
    pub(crate) fn pin(&self) -> Result<ChainPin> {
        ensure!(
            self.version == CHAIN_CONFIG_VERSION
                || (self.version == 1 && self.endpoint_key_base64.is_none()),
            "unsupported pinned chain file version {}",
            self.version
        );
        ensure!(
            !self.chain_id.is_empty() && self.chain_id.len() <= 128,
            "chain ID must hold 1 to 128 bytes"
        );
        Ok(ChainPin {
            network: self.network.into(),
            chain_id: self.chain_id.clone(),
            genesis_digest: hex32(&self.genesis_digest)
                .context("genesis digest must be 64 lowercase hexadecimal characters")?,
        })
    }
}
/// Base units as a token amount with six decimal places.
pub(crate) fn tokens(units: u128) -> String {
    format!("{}.{:06}", units / 1_000_000, units % 1_000_000)
}
pub(crate) fn hex32(raw: &str) -> Result<[u8; 32]> {
    ensure!(
        raw.len() == 64
            && raw
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected 64 lowercase hexadecimal characters"
    );
    let mut out = [0; 32];
    for (index, value) in out.iter_mut().enumerate() {
        *value = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)?;
    }
    Ok(out)
}

/// Node, signer and account selection shared by every consensus command.
#[derive(Debug, Clone, Args)]
pub struct Connection {
    /// A loopback http://IP:PORT node or a remote endpoint's pin file;
    /// defaults to the pinned chain's endpoint.
    #[arg(long)]
    endpoint: Option<String>,
    /// Keystore wallet to sign with; defaults to the active wallet.
    #[arg(long, conflicts_with = "key_file")]
    wallet: Option<String>,
    /// Private JSON key file with algorithm, public_key and private_key.
    #[arg(long)]
    key_file: Option<PathBuf>,
    /// Account address, when the key is not the account's origin key (after
    /// a key rotation). Defaults to the address the key derives.
    #[arg(long)]
    account: Option<String>,
}
/// Node and account selection for reads: an address, or the signer's.
#[derive(Debug, Clone, Args)]
pub struct ReadArgs {
    /// Account address on the pinned network; defaults to the signer's.
    address: Option<String>,
    /// A loopback http://IP:PORT node or a remote endpoint's pin file;
    /// defaults to the pinned chain's endpoint.
    #[arg(long)]
    endpoint: Option<String>,
    /// Keystore wallet whose address to read; defaults to the active wallet.
    #[arg(long, conflicts_with_all = ["key_file", "address"])]
    wallet: Option<String>,
    #[arg(long, conflicts_with = "address")]
    key_file: Option<PathBuf>,
}
impl ReadArgs {
    pub(crate) fn open(&self) -> Result<(ChainPin, CometClient, AccountAddress)> {
        let config = ChainConfig::load()?;
        let pin = config.pin()?;
        let address = match &self.address {
            Some(raw) => AccountAddress::decode(pin.network, raw)
                .map_err(|_| anyhow!("address is not on the pinned network"))?,
            None => {
                let key = load_signer(self.wallet.as_deref(), self.key_file.as_deref())?;
                pin.origin_address(&key_identity(&key)?)?
            }
        };
        let client = config.client(self.endpoint.as_deref())?;
        Ok((pin, client, address))
    }
}
/// The pinned chain and a client for its node, for reads of no account.
pub(crate) fn open_chain(endpoint: Option<&str>) -> Result<(ChainPin, CometClient)> {
    let config = ChainConfig::load()?;
    let pin = config.pin()?;
    let client = config.client(endpoint)?;
    Ok((pin, client))
}

/// Fee and lifetime of one write. The gas limit and fee cap have no default.
#[derive(Debug, Clone, Args)]
pub struct WriteArgs {
    #[arg(long, value_parser = super::ordinary::decimal_u64)]
    gas_limit: u64,
    /// The most uDRT this transaction may be charged.
    #[arg(long, value_parser = super::ordinary::decimal_u128)]
    maximum_fee_udrt: u128,
    /// Blocks after the next one during which the transaction stays valid.
    #[arg(long, default_value = "100", value_parser = super::ordinary::decimal_u64)]
    expiry_blocks: u64,
    #[arg(long, default_value = "")]
    memo: String,
    /// Seconds to wait for commitment; 0 returns once CheckTx admits it.
    #[arg(long, default_value = "30", value_parser = super::ordinary::decimal_u64)]
    wait_seconds: u64,
}

/// The pinned chain, a client for its node and the selected signer.
pub(crate) struct Session {
    pub(crate) pin: ChainPin,
    pub(crate) client: CometClient,
    pub(crate) key: DytallixKeypair,
    pub(crate) identity: KeyIdentity,
    pub(crate) address: AccountAddress,
}
impl Connection {
    pub(crate) fn open(&self) -> Result<Session> {
        let config = ChainConfig::load()?;
        let pin = config.pin()?;
        let client = config.client(self.endpoint.as_deref())?;
        let key = load_signer(self.wallet.as_deref(), self.key_file.as_deref())?;
        let identity = key_identity(&key)?;
        let address = match &self.account {
            Some(raw) => AccountAddress::decode(pin.network, raw)
                .map_err(|_| anyhow!("account is not an address on the pinned network"))?,
            None => pin.origin_address(&identity)?,
        };
        Ok(Session {
            pin,
            client,
            key,
            identity,
            address,
        })
    }
}

/// An explicit wallet or key file, else the keystore's active wallet.
pub(crate) fn load_signer(
    wallet: Option<&str>,
    key_file: Option<&Path>,
) -> Result<DytallixKeypair> {
    if wallet.is_some() || key_file.is_some() {
        return load_signing_key(wallet, key_file);
    }
    let keystore =
        dytallix_sdk::keystore::Keystore::open(dytallix_sdk::keystore::Keystore::default_path())
            .map_err(|_| anyhow!("no keystore; select --wallet or --key-file"))?;
    let name = keystore
        .active()
        .map(|entry| entry.name.clone())
        .context("no active wallet; select --wallet or --key-file")?;
    load_signing_key(Some(&name), None)
}
/// A keystore entry's identity, from its public metadata alone.
pub(crate) fn entry_identity(entry: &dytallix_sdk::KeystoreEntry) -> Result<KeyIdentity> {
    let algorithm = match entry.scheme {
        KeyScheme::MlDsa65 => "mldsa65",
        KeyScheme::MlDsa87 => "mldsa87",
        KeyScheme::SlhDsa => return Err(anyhow!("consensus accounts use ML-DSA-65 or ML-DSA-87")),
    };
    Ok(KeyIdentity {
        algorithm: algorithm.into(),
        public_key: entry.public_key.clone(),
    })
}
pub(crate) fn key_identity(key: &DytallixKeypair) -> Result<KeyIdentity> {
    let algorithm = match key.scheme() {
        KeyScheme::MlDsa65 => "mldsa65",
        KeyScheme::MlDsa87 => "mldsa87",
        KeyScheme::SlhDsa => return Err(anyhow!("consensus accounts use ML-DSA-65 or ML-DSA-87")),
    };
    Ok(KeyIdentity {
        algorithm: algorithm.into(),
        public_key: key.public_key().to_vec(),
    })
}
/// A recipient on the pinned network.
pub(crate) fn recipient(pin: &ChainPin, raw: &str) -> Result<[u8; 32]> {
    Ok(*AccountAddress::decode(pin.network, raw)
        .map_err(|_| anyhow!("recipient is not an address on the pinned network"))?
        .account_id())
}

fn attempts(seconds: u64) -> Result<u32> {
    ensure!(seconds <= 3_600, "--wait-seconds is at most 3600");
    Ok(seconds as u32)
}
fn expiry(committed_height: u64, blocks: u64, max_lifetime: u64) -> Result<u64> {
    ensure!(
        blocks > 0 && blocks <= max_lifetime,
        "--expiry-blocks must be between 1 and the profile's {max_lifetime}"
    );
    committed_height
        .checked_add(1)
        .and_then(|next| next.checked_add(blocks))
        .context("expiry height overflow")
}

/// Prepare, sign and submit one ordinary-v2 transaction, then wait for its
/// committed receipt and check it against the signed envelope.
pub(crate) async fn submit_ordinary(
    session: &Session,
    actions: Vec<Action>,
    write: WriteArgs,
) -> Result<Value> {
    let node = session
        .client
        .signing_context(
            &session.pin,
            session.address.account_id(),
            &session.identity,
        )
        .await?;
    // Rule 4: show the account funded and without a record before signing.
    let first_spend_proof = if node.first_spend {
        let proof = session
            .client
            .prove_first_spend(&session.pin, &session.address)
            .await?;
        Some(
            json!({"height": proof.height, "app_hash": bytes_to_hex(&proof.app_hash),
            "app_hash_source": proof.source}),
        )
    } else {
        None
    };
    let limits = &node.fee_profile.limits;
    let prepared = ordinary::prepare(
        &node.fee_profile,
        &node.context,
        actions,
        write.memo,
        expiry(
            node.context.committed.height,
            write.expiry_blocks,
            limits.max_expiry_lifetime,
        )?,
        write.gas_limit,
        write.maximum_fee_udrt,
    )?;
    let signed = prepared.sign(&KeypairSigner::new(&session.key)?)?;
    let quote = FeeQuote::from_profile(&node.fee_profile, write.gas_limit)?;
    let id = ordinary::transaction_id(&signed.body, limits)?;
    let check = session
        .client
        .submit_sync(&signed, &node.fee_profile, node.max_transport_bytes()?)
        .await?;
    let mut report = json!({"address": session.address.encode(), "transaction_id": bytes_to_hex(&id),
        "first_spend": node.first_spend, "first_spend_proof": first_spend_proof, "maximum_fee_udrt": write.maximum_fee_udrt.to_string(),
        "required_cap_udrt": quote.required_cap.to_string(), "check_tx": check});
    ensure!(
        check.admitted(),
        "CheckTx rejected the transaction ({}): {}",
        check.code,
        check.log
    );
    if write.wait_seconds == 0 {
        report["status"] = "check_tx_accepted".into();
        report["committed"] = false.into();
        return Ok(report);
    }
    match session
        .client
        .wait_for_receipt(&id, attempts(write.wait_seconds)?, WAIT_INTERVAL)
        .await?
    {
        Some(receipt) => {
            ordinary::validate_receipt(&receipt, &signed, &node.fee_profile)?;
            report["status"] = serde_json::to_value(receipt.outcome)?;
            report["committed"] = true.into();
            report["charge_udrt"] = receipt.charge.to_string().into();
            report["receipt"] = serde_json::to_value(&receipt)?;
        }
        None => {
            report["status"] = "not_yet_committed".into();
            report["committed"] = false.into();
        }
    }
    Ok(report)
}

/// Prepare, sign and submit one ordinary-v3 governance transaction, then
/// wait for the spent nonce: the chain writes no v3 receipt yet (gap 12).
pub(crate) async fn submit_governance(
    session: &Session,
    action: impl FnOnce(u64) -> Result<governance::Action>,
    write: WriteArgs,
) -> Result<Value> {
    let node = session
        .client
        .governance_context(
            &session.pin,
            session.address.account_id(),
            &session.identity,
        )
        .await?;
    let limits = node.fee_profile.limits();
    let prepared = governance::prepare(
        &node.fee_profile,
        &node.context,
        action(node.governance.next_proposal_id)?,
        write.memo,
        expiry(
            node.context.committed.height,
            write.expiry_blocks,
            limits.max_expiry_lifetime,
        )?,
        write.gas_limit,
        write.maximum_fee_udrt,
    )?;
    let signed = prepared.sign(&KeypairSigner::new(&session.key)?)?;
    let quote = FeeQuote::from_profile(&node.fee_profile.base, write.gas_limit)?;
    let id = governance::transaction_id(&signed.body, &limits)?;
    let check = session
        .client
        .submit_governance_sync(&signed, &node.fee_profile, node.max_transport_bytes()?)
        .await?;
    let mut report = json!({"address": session.address.encode(), "transaction_id": bytes_to_hex(&id),
        "maximum_fee_udrt": write.maximum_fee_udrt.to_string(), "required_cap_udrt": quote.required_cap.to_string(),
        "rule_failures_are_charged": true, "body": signed.body, "check_tx": check});
    ensure!(
        check.admitted(),
        "CheckTx rejected the transaction ({}): {}",
        check.code,
        check.log
    );
    let spent = if write.wait_seconds == 0 {
        None
    } else {
        session
            .client
            .wait_for_spent_nonce(
                session.address.account_id(),
                signed.body.spending_nonce,
                attempts(write.wait_seconds)?,
                WAIT_INTERVAL,
            )
            .await?
    };
    report["committed"] = spent.is_some().into();
    report["status"] = if spent.is_some() {
        "nonce_spent"
    } else if write.wait_seconds == 0 {
        "check_tx_accepted"
    } else {
        "not_yet_committed"
    }
    .into();
    Ok(report)
}

#[cfg(test)]
#[path = "consensus_tests.rs"]
mod tests;
