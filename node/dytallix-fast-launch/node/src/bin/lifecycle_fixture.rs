//! Local Batch 8 fixture account generation and transaction signing.
//!
//! Every command requires `--fixture-only` as its first argument.
//! Account generation creates six ML-DSA-65 development origin accounts:
//! `lifecycle_fixture --fixture-only accounts --chain batch8-lifecycle
//! --private accounts.json --operators operators.json`
//! The operators file is a public JSON map from validator-0..5 to account address.
//! The private file contains all six fixture accounts and must remain mode 0600.
//!
//! Sign one Msg object or an array of Msg objects from a JSON file:
//! `lifecycle_fixture --fixture-only sign --private accounts.json
//! --validator validator-0 --nonce 0 --messages messages.json --output tx.json`
//! The default fee is 50000 uDRT. Override it with `--fee`.
//! Output is the complete `WireTransaction::Signed` JSON accepted by the bridge.
//!
//! Construct a domain-bound ML-DSA-65 key proof payload:
//! `lifecycle_fixture --fixture-only proof --private accounts.json
//! --validator validator-0 --operation rotate --consensus-pubkey BASE64
//! --nonce 0 --expires-at-height 100 --amount-udgt 0 --output proof.bin`
//! Sign that binary payload with the fixture-only dytallix-comet-proof command.
//! All output files are created exclusively. No command prints private keys.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::{
    addr::{initial_address, AddressNetwork, OriginKeyAlgorithm},
    consensus_settlement::WireTransaction,
    crypto::{canonical_json, sha3_256, ActivePQC, PQC},
    runtime::validator_lifecycle::proof_sign_bytes,
    types::{tx::Tx, Msg, SignedTx},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};

const MARKER: &str = "dytallix-mldsa65-local-fixture-v2";
const MAX_INPUT: u64 = 512 * 1024;
const ACCOUNT_COUNT: usize = 6;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureAccount {
    address: String,
    public_key: String,
    secret_key: String,
}
impl Drop for FixtureAccount {
    fn drop(&mut self) {
        self.secret_key.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureAccounts {
    marker: String,
    chain_id: String,
    algorithm: String,
    accounts: BTreeMap<String, FixtureAccount>,
}

fn validate_chain(chain: &str) -> Result<()> {
    ensure!(
        (chain.starts_with("batch8-")
            || chain.starts_with("batch9-")
            || chain.starts_with("batch10-"))
            && chain != "batch10-"
            && chain.len() > 7
            && chain.len() <= 50
            && chain
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !chain.contains("mainnet")
            && !chain.contains("production"),
        "Only a synthetic batch8-, batch9- or batch10- local chain is accepted"
    );
    Ok(())
}
fn take(args: &mut BTreeMap<String, String>, name: &str) -> Result<String> {
    args.remove(name).with_context(|| format!("Missing {name}"))
}
fn done(args: BTreeMap<String, String>) -> Result<()> {
    ensure!(args.is_empty(), "Unknown or unused command argument");
    Ok(())
}
fn read_file(path: &Path, private: bool) -> Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    ensure!(before.file_type().is_file(), "Input must be a regular file");
    let file = File::open(path)?;
    let opened = file.metadata()?;
    ensure!(opened.is_file(), "Input must be a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == opened.dev() && before.ino() == opened.ino(),
            "Input changed during open"
        );
        if private {
            ensure!(
                opened.mode() & 0o777 == 0o600 && opened.nlink() == 1,
                "Private fixture input must be mode 0600 with one link"
            );
        }
    }
    #[cfg(not(unix))]
    ensure!(
        !private,
        "Private fixture files require Unix permission checks"
    );
    ensure!(
        opened.len() <= MAX_INPUT,
        "Fixture input exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_INPUT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_INPUT,
        "Fixture input exceeds size limit"
    );
    Ok(bytes)
}
fn write_new(path: &str, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("Cannot create fixture output exclusively")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn load_private(path: &str) -> Result<FixtureAccounts> {
    let bytes = Zeroizing::new(read_file(Path::new(path), true)?);
    let fixture: FixtureAccounts = serde_json::from_slice(&bytes)?;
    ensure!(
        fixture.marker == MARKER && fixture.algorithm == "ml_dsa_65",
        "Not a Batch 8 fixture account file"
    );
    validate_chain(&fixture.chain_id)?;
    ensure!(
        fixture.accounts.len() == ACCOUNT_COUNT,
        "Fixture must contain exactly six accounts"
    );
    for index in 0..ACCOUNT_COUNT {
        let account = fixture
            .accounts
            .get(&format!("validator-{index}"))
            .context("Missing fixture account")?;
        let public = B64.decode(&account.public_key)?;
        ensure!(
            public.len() == 1952 && B64.encode(&public) == account.public_key,
            "Invalid ML-DSA-65 public key"
        );
        let address = initial_address(
            AddressNetwork::Development,
            &fixture.chain_id,
            OriginKeyAlgorithm::MlDsa65,
            &public,
        )?;
        ensure!(
            address == account.address,
            "Fixture origin address mismatch"
        );
    }
    Ok(fixture)
}
fn accounts(mut args: BTreeMap<String, String>) -> Result<()> {
    let chain = take(&mut args, "--chain")?;
    let private_path = take(&mut args, "--private")?;
    let operators_path = take(&mut args, "--operators")?;
    done(args)?;
    validate_chain(&chain)?;
    ensure!(
        private_path != operators_path,
        "Private and public output paths must differ"
    );
    let mut records = BTreeMap::new();
    let mut operators = BTreeMap::new();
    for index in 0..ACCOUNT_COUNT {
        let (secret, public) = ActivePQC::keypair();
        let secret = Zeroizing::new(secret);
        let address = initial_address(
            AddressNetwork::Development,
            &chain,
            OriginKeyAlgorithm::MlDsa65,
            &public,
        )?;
        let id = format!("validator-{index}");
        operators.insert(id.clone(), address.clone());
        records.insert(
            id,
            FixtureAccount {
                address,
                public_key: B64.encode(public),
                secret_key: B64.encode(&*secret),
            },
        );
    }
    let fixture = FixtureAccounts {
        marker: MARKER.into(),
        chain_id: chain,
        algorithm: "ml_dsa_65".into(),
        accounts: records,
    };
    let private_bytes = Zeroizing::new(serde_json::to_vec_pretty(&fixture)?);
    write_new(&private_path, &private_bytes)?;
    write_new(&operators_path, &serde_json::to_vec_pretty(&operators)?)?;
    println!("Created six local fixture accounts and the public operator map.");
    Ok(())
}
fn sign(mut args: BTreeMap<String, String>) -> Result<()> {
    let fixture = load_private(&take(&mut args, "--private")?)?;
    let id = take(&mut args, "--validator")?;
    let nonce: u64 = take(&mut args, "--nonce")?.parse()?;
    let message_path = take(&mut args, "--messages")?;
    let output = take(&mut args, "--output")?;
    let fee: u128 = args
        .remove("--fee")
        .unwrap_or_else(|| "50000".into())
        .parse()?;
    done(args)?;
    let account = fixture
        .accounts
        .get(&id)
        .context("Unknown fixture account")?;
    let value: serde_json::Value =
        serde_json::from_slice(&read_file(Path::new(&message_path), false)?)?;
    let messages: Vec<Msg> = if value.is_array() {
        serde_json::from_value(value)?
    } else {
        vec![serde_json::from_value(value)?]
    };
    ensure!(
        !messages.is_empty() && messages.len() <= 8,
        "Fixture requires one to eight messages"
    );
    ensure!(
        messages.iter().all(|m| m.sender() == account.address),
        "Message sender differs from fixture signer"
    );
    let transaction = Tx::new(
        fixture.chain_id.clone(),
        nonce,
        messages,
        fee,
        "Batch 8 local fixture",
    )?;
    let public = B64.decode(&account.public_key)?;
    let secret = Zeroizing::new(B64.decode(&account.secret_key)?);
    ensure!(
        secret.len() == 4032 && B64.encode(&*secret) == account.secret_key,
        "Invalid ML-DSA-65 private key encoding"
    );
    {
        use fips204::traits::SerDes;
        let key_bytes: [u8; fips204::ml_dsa_65::SK_LEN] = secret
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid private key length"))?;
        fips204::ml_dsa_65::PrivateKey::try_from_bytes(key_bytes)
            .map_err(|_| anyhow::anyhow!("Invalid fixture private key"))?;
    }
    let envelope = SignedTx::sign(transaction, &secret, &public)?;
    let hash = sha3_256(&canonical_json(&envelope.tx)?);
    ensure!(
        ActivePQC::verify(&public, &hash, &B64.decode(&envelope.signature)?),
        "Fixture signature self-check failed"
    );
    write_new(
        &output,
        &serde_json::to_vec(&WireTransaction::Signed { envelope })?,
    )?;
    println!("Created signed local fixture transaction.");
    Ok(())
}
fn proof(mut args: BTreeMap<String, String>) -> Result<()> {
    let fixture = load_private(&take(&mut args, "--private")?)?;
    let id = take(&mut args, "--validator")?;
    let operation = take(&mut args, "--operation")?;
    let key = take(&mut args, "--consensus-pubkey")?;
    let nonce: u64 = take(&mut args, "--nonce")?.parse()?;
    let expiry: u64 = take(&mut args, "--expires-at-height")?.parse()?;
    let amount: u128 = take(&mut args, "--amount-udgt")?.parse()?;
    let output = take(&mut args, "--output")?;
    done(args)?;
    ensure!(expiry > 0, "Proof expiry must be positive");
    ensure!(
        (operation == "register" && amount > 0) || (operation == "rotate" && amount == 0),
        "Invalid proof operation or amount"
    );
    let account = fixture
        .accounts
        .get(&id)
        .context("Unknown fixture account")?;
    let bytes = proof_sign_bytes(
        &fixture.chain_id,
        &operation,
        &id,
        &account.address,
        &key,
        nonce,
        expiry,
        amount,
    )?;
    write_new(&output, &bytes)?;
    println!("Created local fixture consensus key proof payload.");
    Ok(())
}
fn main() -> Result<()> {
    ensure!(
        cfg!(feature = "pqc-fips204"),
        "This helper requires the FIPS 204 transaction backend"
    );
    let mut input = std::env::args().skip(1);
    ensure!(
        input.next().as_deref() == Some("--fixture-only"),
        "First argument must be --fixture-only"
    );
    let command = input
        .next()
        .context("Expected accounts, sign, or proof command")?;
    let mut args = BTreeMap::new();
    while let Some(name) = input.next() {
        ensure!(
            name.starts_with("--") && name.len() <= 40,
            "Expected named argument"
        );
        let value = input.next().context("Missing argument value")?;
        ensure!(value.len() <= 8192, "Argument exceeds fixture limit");
        ensure!(args.insert(name, value).is_none(), "Duplicate argument");
        ensure!(args.len() <= 12, "Too many arguments");
    }
    match command.as_str() {
        "accounts" => accounts(args),
        "sign" => sign(args),
        "proof" => proof(args),
        _ => bail!("Expected accounts, sign, or proof command"),
    }
}
