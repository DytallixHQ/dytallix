//! Explicit ordinary-v2 commands. No legacy endpoint or identity defaults apply.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use clap::{Args, Subcommand};
use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
use dytallix_sdk::ordinary_client::CometClient;
use dytallix_sdk::ordinary_v2::{
    self as ordinary, AccountAddress, AccountView, Action, AddressNetwork, FeeProfile, FeeQuote,
    KeyIdentity, KeypairSigner, OrdinaryTransaction, OriginKeyAlgorithm, PreparedTransaction,
    ProfileView, RecoveryDomain, SignedOrdinary, SigningContext,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::bytes_to_hex;

const MAX_FILE_BYTES: usize = 1_048_576;
const MAX_RESPONSE_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Args)]
pub struct OrdinaryArgs {
    #[command(subcommand)]
    pub command: OrdinaryCommand,
}

#[derive(Debug, Clone, Args)]
pub struct CapturedInputs {
    /// Captured committed profile query JSON.
    #[arg(long)]
    profile: PathBuf,
    /// Captured committed account query JSON.
    #[arg(long)]
    account: PathBuf,
    /// Explicit expected domain, authority, counters, and committed state.
    #[arg(long)]
    context: PathBuf,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrdinaryCommand {
    /// Prepare an unsigned public body offline. This does not reserve funds.
    Prepare {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long)]
        actions: PathBuf,
        #[arg(long)]
        memo: String,
        #[arg(long, value_parser = decimal_u64)]
        expiry_height: u64,
        #[arg(long, value_parser = decimal_u64)]
        gas_limit: u64,
        #[arg(long, value_parser = decimal_u128)]
        maximum_fee_udrt: u128,
        #[arg(long)]
        output: PathBuf,
    },
    /// Sign an exact body offline with an explicitly selected existing key.
    Sign {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long)]
        body: PathBuf,
        /// Existing named keystore entry. Its legacy address is not used.
        #[arg(
            long,
            required_unless_present = "key_file",
            conflicts_with = "key_file"
        )]
        wallet: Option<String>,
        /// Private JSON file with algorithm, public_key, and private_key byte arrays.
        #[arg(long, required_unless_present = "wallet", conflicts_with = "wallet")]
        key_file: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate a body or signature against captured state and display public fields.
    Inspect {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long, required_unless_present = "signed", conflicts_with = "signed")]
        body: Option<PathBuf>,
        #[arg(long, required_unless_present = "body", conflicts_with = "body")]
        signed: Option<PathBuf>,
    },
    /// Write the canonical node transport offline. This does not submit it.
    Transport {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long)]
        signed: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Read the committed ordinary profile from an explicit Comet RPC endpoint.
    QueryProfile {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Read committed account authority from an explicit Comet RPC endpoint.
    QueryAccount {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        account_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Read and check a committed receipt. An absent receipt is not success.
    Receipt {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        transaction_id: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        signed: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Submit an exact signed transaction. CheckTx acceptance is not commitment.
    Submit {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        signed: PathBuf,
    },
    /// Check one initial identity against exact genesis bytes and captured state.
    CheckGenesis {
        #[command(flatten)]
        inputs: CapturedInputs,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        genesis_file: PathBuf,
    },
}

struct ValidatedInputs {
    profile: FeeProfile,
    view: ProfileView,
    account: AccountView,
    context: SigningContext,
}
impl CapturedInputs {
    fn load(&self) -> Result<ValidatedInputs> {
        let view = read_json(&self.profile, false)?;
        let account = read_json(&self.account, false)?;
        let context = read_json(&self.context, false)?;
        let profile = ordinary::validate_views(&context, &view, &account)?;
        Ok(ValidatedInputs {
            profile,
            view,
            account,
            context,
        })
    }
    fn load_identity(&self) -> Result<ValidatedInputs> {
        let view = read_json(&self.profile, false)?;
        let account = read_json(&self.account, false)?;
        let context = read_json(&self.context, false)?;
        let profile = ordinary::validate_identity_views(&context, &view, &account)?;
        Ok(ValidatedInputs {
            profile,
            view,
            account,
            context,
        })
    }
}
impl ValidatedInputs {
    fn body(&self, body: OrdinaryTransaction) -> Result<PreparedTransaction> {
        Ok(PreparedTransaction::from_body(
            body,
            &self.profile,
            &self.context,
        )?)
    }
    fn signed(&self, path: &Path) -> Result<SignedOrdinary> {
        let signed: SignedOrdinary = read_json(path, false)?;
        self.body(signed.body.clone())?;
        ordinary::verify_signature(&signed, &self.profile.limits)?;
        Ok(signed)
    }
    fn transport_bound(&self) -> Result<usize> {
        let config = self
            .view
            .config
            .as_ref()
            .context("ordinary configuration is missing")?;
        Ok(usize::try_from(config.max_transport_bytes)?)
    }
}

pub async fn run(args: OrdinaryArgs) -> Result<()> {
    match args.command {
        OrdinaryCommand::Prepare {
            inputs,
            actions,
            memo,
            expiry_height,
            gas_limit,
            maximum_fee_udrt,
            output,
        } => {
            let inputs = inputs.load()?;
            let actions: Vec<Action> = read_json(&actions, false)?;
            let prepared = ordinary::prepare(
                &inputs.profile,
                &inputs.context,
                actions,
                memo,
                expiry_height,
                gas_limit,
                maximum_fee_udrt,
            )?;
            write_json(&output, prepared.body())?;
            print_json(&describe(
                prepared.body(),
                &inputs.profile,
                "prepared_offline",
            )?)?;
        }
        OrdinaryCommand::Sign {
            inputs,
            body,
            wallet,
            key_file,
            output,
        } => {
            let inputs = inputs.load()?;
            let prepared = inputs.body(read_json(&body, false)?)?;
            let key = load_signing_key(wallet.as_deref(), key_file.as_deref())?;
            let signed = prepared.sign(&KeypairSigner::new(&key)?)?;
            write_json(&output, &signed)?;
            print_json(&describe(&signed.body, &inputs.profile, "signed_offline")?)?;
        }
        OrdinaryCommand::Inspect {
            inputs,
            body,
            signed,
        } => {
            let inputs = inputs.load()?;
            let (body, status) = match (body, signed) {
                (Some(path), None) => (
                    inputs.body(read_json(&path, false)?)?.body().clone(),
                    "body_checked_offline",
                ),
                (None, Some(path)) => (inputs.signed(&path)?.body, "signature_checked_offline"),
                _ => return Err(anyhow!("select exactly one body or signed file")),
            };
            print_json(&describe(&body, &inputs.profile, status)?)?;
        }
        OrdinaryCommand::Transport {
            inputs,
            signed,
            output,
        } => {
            let inputs = inputs.load()?;
            let signed = inputs.signed(&signed)?;
            let transport = ordinary::encode_transport(
                &signed,
                &inputs.profile.limits,
                inputs.transport_bound()?,
            )?;
            write_new(&output, &transport)?;
            print_json(&describe(
                &signed.body,
                &inputs.profile,
                "transport_prepared_offline",
            )?)?;
        }
        OrdinaryCommand::QueryProfile { endpoint, output } => {
            let value = client(&endpoint)?.query_profile().await?;
            write_json(&output, &value)?;
            print_json(&value)?;
        }
        OrdinaryCommand::QueryAccount {
            endpoint,
            account_id,
            output,
        } => {
            let value = client(&endpoint)?
                .query_account(&canonical_id(&account_id)?)
                .await?
                .context("account is absent from committed state")?;
            write_json(&output, &value)?;
            print_json(&value)?;
        }
        OrdinaryCommand::Receipt {
            endpoint,
            transaction_id,
            profile,
            signed,
            output,
        } => {
            let view: ProfileView = read_json(&profile, false)?;
            let profile = &view
                .config
                .as_ref()
                .context("ordinary profile is missing")?
                .fee_profile;
            let signed: SignedOrdinary = read_json(&signed, false)?;
            ordinary::verify_signature(&signed, &profile.limits)?;
            let id = canonical_id(&transaction_id)?;
            ensure!(
                id == ordinary::transaction_id(&signed.body, &profile.limits)?,
                "requested receipt ID differs from signed body"
            );
            let receipt = client(&endpoint)?.query_receipt(&id).await?;
            if let Some(receipt) = receipt {
                ordinary::validate_receipt(&receipt, &signed, profile)?;
                if let Some(output) = output {
                    write_json(&output, &receipt)?;
                }
                print_json(
                    &json!({"status":"committed_receipt_reported", "consensus_proof_verified":false, "receipt":receipt}),
                )?;
            } else {
                print_json(
                    &json!({"status":"receipt_absent", "transaction_id":transaction_id, "committed":false, "actual_charge_udrt":null}),
                )?;
            }
        }
        OrdinaryCommand::Submit {
            inputs,
            endpoint,
            signed,
        } => {
            let inputs = inputs.load()?;
            let signed = inputs.signed(&signed)?;
            let client = client(&endpoint)?;
            // Refresh both views. Matching data is not a consensus proof.
            let profile = client.query_profile().await?;
            let account = client
                .query_account(&inputs.context.domain.account_id)
                .await?
                .context("account is absent from committed state")?;
            ordinary::validate_views(&inputs.context, &profile, &account)?;
            let result = client
                .submit_sync(&signed, &inputs.profile, inputs.transport_bound()?)
                .await?;
            print_json(
                &json!({"status":if result.admitted() {"check_tx_accepted"} else {"check_tx_rejected"},
                "committed":false, "actual_charge_udrt":null, "check_tx":result}),
            )?;
            ensure!(
                result.admitted(),
                "CheckTx rejected the transaction; no committed result is claimed"
            );
        }
        OrdinaryCommand::CheckGenesis {
            inputs,
            manifest,
            genesis_file,
        } => {
            let inputs = inputs.load_identity()?;
            let manifest = read_json(&manifest, false)?;
            let genesis = read_bounded(&genesis_file, false)?;
            check_genesis(&inputs, &manifest, &genesis)?;
            print_json(
                &json!({"status":"genesis_identity_compatible", "account_id":bytes_to_hex(&inputs.context.domain.account_id),
                "genesis_digest":bytes_to_hex(&inputs.context.domain.genesis_digest), "full_genesis_validated":false}),
            )?;
        }
    }
    Ok(())
}

fn client(endpoint: &str) -> Result<CometClient> {
    Ok(CometClient::new(endpoint, MAX_RESPONSE_BYTES)?)
}

fn describe(body: &OrdinaryTransaction, profile: &FeeProfile, status: &str) -> Result<Value> {
    let quote = FeeQuote::from_profile(profile, body.gas_limit)?;
    Ok(
        json!({"status":status, "transaction_id":bytes_to_hex(&ordinary::transaction_id(body, &profile.limits)?),
        "maximum_fee_udrt":body.maximum_fee.to_string(), "required_cap_udrt":quote.required_cap.to_string(),
        "minimum_charge_udrt":quote.minimum_charge.to_string(), "actual_charge_udrt":null,
        "committed":false, "body":body}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateKeyInput {
    algorithm: String,
    public_key: Vec<u8>,
    private_key: Vec<u8>,
}
// Read the existing keystore format once. This command never imports or saves it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingKeystore {
    #[serde(rename = "active")]
    _active: Option<String>,
    entries: Vec<dytallix_sdk::KeystoreEntry>,
}
fn exact_scheme(algorithm: &str) -> Result<KeyScheme> {
    match algorithm {
        "mldsa65" => Ok(KeyScheme::MlDsa65),
        #[cfg(not(feature = "strict-local-mldsa65"))]
        "mldsa87" => Ok(KeyScheme::MlDsa87),
        #[cfg(not(feature = "strict-local-mldsa65"))]
        _ => Err(anyhow!("algorithm must be exactly mldsa65 or mldsa87")),
        #[cfg(feature = "strict-local-mldsa65")]
        _ => Err(anyhow!("strict local profile requires exactly mldsa65")),
    }
}
fn load_signing_key(wallet: Option<&str>, key_file: Option<&Path>) -> Result<DytallixKeypair> {
    match (wallet, key_file) {
        (None, Some(path)) => {
            let input: PrivateKeyInput = read_json(path, true)?;
            DytallixKeypair::from_keypair(
                exact_scheme(&input.algorithm)?,
                &input.public_key,
                &input.private_key,
            )
            .map_err(|_| anyhow!("private key file does not contain a valid matching key pair"))
        }
        (Some(name), None) => {
            let keystore: ExistingKeystore =
                read_json(&dytallix_sdk::keystore::Keystore::default_path(), true)?;
            let mut entries = keystore.entries.iter().filter(|entry| entry.name == name);
            let entry = entries.next().context("named wallet does not exist")?;
            ensure!(
                entries.next().is_none(),
                "selected wallet name is duplicated"
            );
            ensure!(
                matches!(entry.scheme, KeyScheme::MlDsa65 | KeyScheme::MlDsa87),
                "ordinary signing requires ML-DSA-65 or ML-DSA-87"
            );
            DytallixKeypair::from_keypair(entry.scheme, &entry.public_key, &entry.private_key)
                .map_err(|_| anyhow!("wallet does not contain a valid matching key pair"))
        }
        _ => Err(anyhow!("select exactly one wallet or private key file")),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GenesisIdentity {
    domain: RecoveryDomain,
    address: String,
    origin_key: KeyIdentity,
    current_key: KeyIdentity,
    fee_profile_digest: [u8; 32],
}
fn check_genesis(inputs: &ValidatedInputs, manifest: &GenesisIdentity, bytes: &[u8]) -> Result<()> {
    ensure!(
        inputs.context.committed.height == 0,
        "genesis identity requires committed height zero"
    );
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    ensure!(
        digest == manifest.domain.genesis_digest,
        "exact genesis file digest differs from manifest"
    );
    ensure!(
        manifest.domain == inputs.context.domain
            && manifest.current_key == inputs.context.current_key
            && manifest.fee_profile_digest == inputs.context.profile_digest,
        "genesis manifest differs from initial captured identity"
    );
    let network = match manifest.domain.network {
        1 => AddressNetwork::Mainnet,
        2 => AddressNetwork::Testnet,
        3 => AddressNetwork::Development,
        _ => return Err(anyhow!("unsupported network")),
    };
    let algorithm = match exact_scheme(&manifest.origin_key.algorithm)? {
        KeyScheme::MlDsa65 => OriginKeyAlgorithm::MlDsa65,
        KeyScheme::MlDsa87 => OriginKeyAlgorithm::MlDsa87,
        KeyScheme::SlhDsa => unreachable!(),
    };
    let origin = AccountAddress::from_origin_key(
        network,
        &manifest.domain.chain_id,
        algorithm,
        &manifest.origin_key.public_key,
    )?;
    let address = AccountAddress::decode(network, &manifest.address)?;
    ensure!(
        origin == address
            && address.account_id() == &manifest.domain.account_id
            && manifest.address == inputs.account.address,
        "genesis origin or address differs"
    );
    // Only identity membership is checked here. The node validates the full document.
    let document: GenesisMembership =
        serde_json::from_slice(bytes).context("invalid genesis membership JSON")?;
    ensure!(
        document.chain_id == manifest.domain.chain_id,
        "genesis chain ID differs"
    );
    ensure!(
        document
            .accounts
            .iter()
            .filter(|a| a.address == manifest.address)
            .count()
            == 1,
        "genesis must contain the manifest address exactly once"
    );
    Ok(())
}

#[derive(Deserialize)]
struct GenesisMembership {
    chain_id: String,
    accounts: Vec<GenesisMember>,
}
#[derive(Deserialize)]
struct GenesisMember {
    address: String,
}

fn read_bounded(path: &Path, private: bool) -> Result<Vec<u8>> {
    let inspected = fs::symlink_metadata(path).context("cannot inspect input file")?;
    ensure!(
        inspected.file_type().is_file(),
        "input must be a regular file, not a symlink"
    );
    let file = File::open(path).context("cannot open input file")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_FILE_BYTES as u64,
        "input file exceeds the 1 MiB limit"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        ensure!(
            inspected.dev() == metadata.dev() && inspected.ino() == metadata.ino(),
            "input file changed while opening"
        );
        if private {
            ensure!(
                metadata.permissions().mode() & 0o077 == 0,
                "private key file must deny group and other access"
            );
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "input file exceeds the 1 MiB limit"
    );
    Ok(bytes)
}
fn read_json<T: DeserializeOwned>(path: &Path, private: bool) -> Result<T> {
    let bytes = read_bounded(path, private)?;
    // Do not include deserializer errors: malformed private fields may appear in them.
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("input file does not match the required JSON schema"))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "output exceeds the 1 MiB limit"
    );
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("cannot create output; an existing file will not be replaced")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_new(path, &serde_json::to_vec_pretty(value)?)
}
fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}
fn decimal_u128(raw: &str) -> std::result::Result<u128, String> {
    if raw.is_empty()
        || !raw.bytes().all(|b| b.is_ascii_digit())
        || (raw.len() > 1 && raw.starts_with('0'))
    {
        return Err("use canonical unsigned decimal base units".into());
    }
    raw.parse().map_err(|_| "decimal value exceeds u128".into())
}
fn decimal_u64(raw: &str) -> std::result::Result<u64, String> {
    u64::try_from(decimal_u128(raw)?).map_err(|_| "decimal value exceeds u64".into())
}
fn canonical_id(raw: &str) -> Result<[u8; 32]> {
    ensure!(
        raw.len() == 64
            && raw
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "ID requires exactly 64 lowercase hexadecimal characters"
    );
    let mut id = [0; 32];
    for (index, value) in id.iter_mut().enumerate() {
        *value = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)?;
    }
    Ok(id)
}

#[cfg(test)]
#[path = "ordinary_tests.rs"]
mod tests;
