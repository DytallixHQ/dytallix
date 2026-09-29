//! Recovery transactions (E04 gap 17, T-c2; P01 29 September 2026). Each
//! party signs on their own machine:
//!
//!   query -> prepare -> sign (each signer) -> assemble -> sponsor -> submit -> receipt
//!
//! Every step but `query`, `submit` and `receipt` works offline from files.
//! A separate sponsor account pays; the recovering account never does.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Args, Subcommand, ValueEnum};
use dytallix_sdk::recovery::{
    self, KeyIdentity, RecoveryAccountView, RecoveryRequest, RecoverySignature, Requirements,
    SignatureRole, SponsorBounds,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ordinary::{client, decimal_u128, load_signing_key, print_json, read_json, write_json};

/// Version of the files these commands write (interfaces v1).
pub(crate) const FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Args)]
pub struct RecoveryArgs {
    #[command(subcommand)]
    pub command: RecoveryCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Role {
    /// Approve the operation (active key, guardians, replacement).
    Operation,
    /// Prove possession of a key the operation installs.
    Possession,
}
impl From<Role> for SignatureRole {
    fn from(role: Role) -> Self {
        match role {
            Role::Operation => SignatureRole::Operation,
            Role::Possession => SignatureRole::Possession,
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum RecoveryCommand {
    /// Read an account's recovery view from an explicit Comet RPC endpoint.
    Query {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        account_id: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Prepare an unsigned operation from a captured view, offline.
    Prepare {
        /// The account's captured recovery view.
        #[arg(long)]
        view: PathBuf,
        /// The action, as JSON: {"action":"start","replacement":{...}} and so on.
        #[arg(long)]
        request: PathBuf,
        /// The height the operation must be included before.
        #[arg(long)]
        expiry_height: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Sign a prepared operation with one key, offline.
    Sign {
        #[arg(long)]
        operation: PathBuf,
        #[arg(long, value_enum)]
        role: Role,
        /// Existing named keystore entry.
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
    /// Check every signature and the requirements, and order them, offline.
    Assemble {
        #[arg(long)]
        operation: PathBuf,
        /// A signature file; repeat for each signer.
        #[arg(long = "signature", required = true)]
        signatures: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Add the sponsor account's signature and fee bounds, offline.
    Sponsor {
        #[arg(long)]
        signed: PathBuf,
        /// The recovering account's captured view.
        #[arg(long)]
        target_view: PathBuf,
        /// The paying account's captured view.
        #[arg(long)]
        sponsor_view: PathBuf,
        #[arg(
            long,
            required_unless_present = "key_file",
            conflicts_with = "key_file"
        )]
        wallet: Option<String>,
        #[arg(long, required_unless_present = "wallet", conflicts_with = "wallet")]
        key_file: Option<PathBuf>,
        #[arg(long)]
        gas_limit: u64,
        #[arg(long, value_parser = decimal_u128)]
        maximum_charge_udrt: u128,
        /// The height the sponsor's authorization must be included before.
        #[arg(long)]
        expiry_height: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit a sponsored transaction. CheckTx acceptance is not commitment.
    Submit {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        transaction: PathBuf,
    },
    /// Read the transaction's sponsor receipt, verified against the pinned
    /// chain's application hash. An absent receipt is not success.
    Receipt {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        transaction: PathBuf,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationFile {
    file_version: u32,
    kind: String,
    operation_id: String,
    operation_base64: String,
    requirements: Requirements,
    request: RecoveryRequest,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureFile {
    file_version: u32,
    kind: String,
    operation_id: String,
    role: String,
    key: KeyIdentity,
    signature_base64: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedFile {
    file_version: u32,
    kind: String,
    operation_id: String,
    signed_base64: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionFile {
    file_version: u32,
    kind: String,
    operation_id: String,
    authorization_id: String,
    envelope_base64: String,
}

fn decode64(value: &str) -> Result<Vec<u8>> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| anyhow!("invalid base64 in the input file"))?;
    ensure!(STANDARD.encode(&bytes) == value, "base64 is not canonical");
    Ok(bytes)
}
fn check_kind(version: u32, kind: &str, expected: &str) -> Result<()> {
    ensure!(
        version == FILE_VERSION && kind == expected,
        "expected a version {FILE_VERSION} {expected} file"
    );
    Ok(())
}
fn role_name(role: SignatureRole) -> &'static str {
    match role {
        SignatureRole::Operation => "operation",
        SignatureRole::Possession => "possession",
    }
}
fn load_operation(path: &Path) -> Result<(OperationFile, recovery::RecoveryOperation)> {
    let file: OperationFile = read_json(path, false)?;
    check_kind(file.file_version, &file.kind, "dytallix-recovery-operation")?;
    let operation = recovery::decode_unsigned(&decode64(&file.operation_base64)?)?;
    ensure!(
        recovery::hex(&recovery::operation_id(&operation)?) == file.operation_id,
        "the operation file's ID differs from its operation"
    );
    Ok((file, operation))
}
fn load_view(path: &Path) -> Result<RecoveryAccountView> {
    let view: RecoveryAccountView = read_json(path, false)?;
    ensure!(view.version == 1, "unsupported recovery view version");
    Ok(view)
}
fn load_transaction(path: &Path) -> Result<(TransactionFile, recovery::SponsoredRecovery)> {
    let file: TransactionFile = read_json(path, false)?;
    check_kind(
        file.file_version,
        &file.kind,
        "dytallix-recovery-transaction",
    )?;
    let sponsored = recovery::decode_envelope(&decode64(&file.envelope_base64)?)?;
    ensure!(
        recovery::hex(&recovery::authorization_id(&sponsored)?) == file.authorization_id,
        "the transaction file's authorization ID differs from its envelope"
    );
    Ok((file, sponsored))
}

pub async fn run(args: RecoveryArgs) -> Result<()> {
    match args.command {
        RecoveryCommand::Query {
            endpoint,
            account_id,
            output,
        } => {
            let id = recovery::parse_hex32(&account_id)?;
            let view = client(&endpoint)?
                .query_recovery_account(&id)
                .await?
                .context("the account has no recovery record")?;
            write_json(&output, &view)?;
            print_json(
                &json!({"status":"recovery_view_captured", "account_id":account_id,
                "height":view.context.height, "recovery_status":view.status}),
            )?;
        }
        RecoveryCommand::Prepare {
            view,
            request,
            expiry_height,
            output,
        } => {
            let view = load_view(&view)?;
            let request: RecoveryRequest = read_json(&request, false)?;
            let prepared = recovery::build(&view, &request, expiry_height)?;
            let operation_id = recovery::hex(&recovery::operation_id(&prepared.operation)?);
            write_json(
                &output,
                &OperationFile {
                    file_version: FILE_VERSION,
                    kind: "dytallix-recovery-operation".into(),
                    operation_id: operation_id.clone(),
                    operation_base64: STANDARD
                        .encode(recovery::unsigned_bytes(&prepared.operation)?),
                    requirements: prepared.requirements.clone(),
                    request,
                },
            )?;
            print_json(
                &json!({"status":"operation_prepared", "operation_id":operation_id,
                "requirements":prepared.requirements}),
            )?;
        }
        RecoveryCommand::Sign {
            operation,
            role,
            wallet,
            key_file,
            output,
        } => {
            let (file, operation) = load_operation(&operation)?;
            let key = load_signing_key(wallet.as_deref(), key_file.as_deref())?;
            let signature = recovery::sign(&operation, role.into(), &key)?;
            let expected = match signature.role {
                SignatureRole::Operation => {
                    file.requirements.operation_keys.contains(&signature.key)
                        || file.requirements.guardians.contains(&signature.key)
                }
                SignatureRole::Possession => {
                    file.requirements.possession_keys.contains(&signature.key)
                }
            };
            ensure!(
                expected,
                "this key is not named for the {} role of the operation",
                role_name(signature.role)
            );
            write_json(
                &output,
                &SignatureFile {
                    file_version: FILE_VERSION,
                    kind: "dytallix-recovery-signature".into(),
                    operation_id: file.operation_id.clone(),
                    role: role_name(signature.role).into(),
                    key: signature.key,
                    signature_base64: STANDARD.encode(&signature.signature),
                },
            )?;
            print_json(
                &json!({"status":"operation_signed", "operation_id":file.operation_id,
                "role":role_name(role.into())}),
            )?;
        }
        RecoveryCommand::Assemble {
            operation,
            signatures,
            output,
        } => {
            let (file, operation) = load_operation(&operation)?;
            let mut collected = Vec::new();
            for path in &signatures {
                let signature: SignatureFile = read_json(path, false)?;
                check_kind(
                    signature.file_version,
                    &signature.kind,
                    "dytallix-recovery-signature",
                )?;
                ensure!(
                    signature.operation_id == file.operation_id,
                    "{} signs another operation",
                    path.display()
                );
                let role = match signature.role.as_str() {
                    "operation" => SignatureRole::Operation,
                    "possession" => SignatureRole::Possession,
                    _ => return Err(anyhow!("unknown signature role in {}", path.display())),
                };
                collected.push(RecoverySignature {
                    role,
                    key: signature.key,
                    signature: decode64(&signature.signature_base64)?,
                });
            }
            let signed = recovery::assemble(&operation, &file.requirements, collected)?;
            write_json(
                &output,
                &SignedFile {
                    file_version: FILE_VERSION,
                    kind: "dytallix-recovery-signed".into(),
                    operation_id: file.operation_id.clone(),
                    signed_base64: STANDARD.encode(recovery::signed_bytes(&signed)?),
                },
            )?;
            print_json(
                &json!({"status":"operation_assembled", "operation_id":file.operation_id,
                "signatures":signed.signatures.len()}),
            )?;
        }
        RecoveryCommand::Sponsor {
            signed,
            target_view,
            sponsor_view,
            wallet,
            key_file,
            gas_limit,
            maximum_charge_udrt,
            expiry_height,
            output,
        } => {
            let file: SignedFile = read_json(&signed, false)?;
            check_kind(file.file_version, &file.kind, "dytallix-recovery-signed")?;
            let signed = recovery::decode_signed(&decode64(&file.signed_base64)?)?;
            ensure!(
                recovery::hex(&recovery::operation_id(&signed.operation)?) == file.operation_id,
                "the signed file's ID differs from its operation"
            );
            let key = load_signing_key(wallet.as_deref(), key_file.as_deref())?;
            let sponsored = recovery::sponsor(
                &signed,
                &load_view(&target_view)?,
                &load_view(&sponsor_view)?,
                &key,
                SponsorBounds {
                    gas_limit,
                    maximum_charge: maximum_charge_udrt,
                    expiry_height,
                },
            )?;
            let authorization_id = recovery::hex(&recovery::authorization_id(&sponsored)?);
            write_json(
                &output,
                &TransactionFile {
                    file_version: FILE_VERSION,
                    kind: "dytallix-recovery-transaction".into(),
                    operation_id: file.operation_id.clone(),
                    authorization_id: authorization_id.clone(),
                    envelope_base64: STANDARD.encode(recovery::envelope_bytes(&sponsored)?),
                },
            )?;
            print_json(
                &json!({"status":"transaction_sponsored", "operation_id":file.operation_id,
                "authorization_id":authorization_id, "maximum_charge_udrt":maximum_charge_udrt.to_string()}),
            )?;
        }
        RecoveryCommand::Submit {
            endpoint,
            transaction,
        } => {
            let (file, sponsored) = load_transaction(&transaction)?;
            let result = client(&endpoint)?.submit_recovery_sync(&sponsored).await?;
            print_json(
                &json!({"status":if result.admitted() {"check_tx_accepted"} else {"check_tx_rejected"},
                "authorization_id":file.authorization_id, "committed":false, "check_tx":result}),
            )?;
            ensure!(
                result.admitted(),
                "CheckTx rejected the transaction; no committed result is claimed"
            );
        }
        RecoveryCommand::Receipt {
            endpoint,
            transaction,
        } => {
            let (file, sponsored) = load_transaction(&transaction)?;
            let chain = crate::commands::consensus::ChainConfig::load()?;
            let pin = chain.pin()?;
            let verified = client(&endpoint)?
                .query_recovery_receipt(&pin, &recovery::authorization_id(&sponsored)?, None)
                .await?;
            match verified.value {
                Some(bytes) => {
                    let receipt: serde_json::Value = serde_json::from_slice(&bytes)
                        .map_err(|_| anyhow!("the receipt is not JSON"))?;
                    print_json(
                        &json!({"status":"committed_receipt_verified", "authorization_id":file.authorization_id,
                        "height":verified.height, "receipt":receipt}),
                    )?;
                }
                None => print_json(
                    &json!({"status":"receipt_absent", "authorization_id":file.authorization_id,
                    "height":verified.height, "committed":false}),
                )?,
            }
        }
    }
    Ok(())
}
