//! Disposable client qualification fixture. No production input or key is read.
//! The normal test validates exact serialized genesis through the real node.
//! Export requires an explicitly supplied, existing, empty output directory.
use super::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const EXPORT_DIRECTORY: &str = "DYTX_ORDINARY_CLIENT_FIXTURE_DIR";

struct ClientKey {
    algorithm: &'static str,
    public: Vec<u8>,
    private: Vec<u8>,
    address: String,
    id: [u8; 32],
}
impl ClientKey {
    fn from_operational(key: &Key) -> Self {
        Self {
            algorithm: "mldsa65",
            public: key.identity.public_key.clone(),
            private: key.secret.clone(),
            address: key.address(),
            id: key.id(),
        }
    }
    fn fresh_65() -> Self {
        let (public, private) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let public = public.into_bytes().to_vec();
        let address = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            CHAIN,
            crate::addr::OriginKeyAlgorithm::MlDsa65,
            &public,
        )
        .unwrap();
        let id = *AccountAddress::decode(AddressNetwork::Development, &address)
            .unwrap()
            .account_id();
        Self {
            algorithm: "mldsa65",
            public,
            private: private.into_bytes().to_vec(),
            address,
            id,
        }
    }
    fn identity(&self) -> KeyIdentity {
        KeyIdentity {
            algorithm: self.algorithm.into(),
            public_key: self.public.clone(),
        }
    }
    fn public_view(&self) -> Value {
        json!({"algorithm":self.algorithm,"public_key":self.public,
            "account_id":hex::encode(self.id),"address":self.address})
    }
    fn cli_key(&self) -> Value {
        // This is the strict ordinary CLI key-file schema. Do not add fields.
        json!({"algorithm":self.algorithm,"public_key":self.public,"private_key":self.private})
    }
}

/// Replace the unused secondary account. A fourth large PQC key could exceed
/// consensus_stdio's existing 65,536-byte configuration transport bound.
fn fixture_with_operational_keys() -> (Fixture, BTreeMap<&'static str, ClientKey>) {
    let mut fixture = Fixture::new();
    let actor65 = ClientKey::fresh_65();
    let old_id = hex::encode(fixture.secondary.id());
    let mut genesis: Value = serde_json::from_slice(&fixture.genesis).unwrap();
    let accounts = genesis["accounts"].as_array_mut().unwrap();
    let secondary = accounts
        .iter_mut()
        .find(|account| account["address"] == fixture.secondary.address())
        .unwrap();
    secondary["address"] = Value::String(actor65.address.clone());
    fixture.genesis = serde_json::to_vec(&genesis).unwrap();
    let digest: [u8; 32] = Sha256::digest(&fixture.genesis).into();
    fixture.config.app_state_sha256 = hex::encode(digest);
    let book = fixture.config.recovery.as_mut().unwrap();
    let old = book.accounts.remove(&old_id).unwrap();
    book.profile.signature_costs.insert("mldsa65".into(), 1);
    for account in book.accounts.values_mut() {
        account.recovery.domain.genesis_digest = digest;
        account
            .recovery
            .config
            .algorithms
            .insert("mldsa65".into(), 1952);
    }
    let mut recovery_config = old.recovery.config.clone();
    recovery_config.algorithms.insert("mldsa65".into(), 1952);
    book.accounts.insert(
        hex::encode(actor65.id),
        RecoveryAccount {
            address: actor65.address.clone(),
            sponsor_nonce: 0,
            recovery: RecoveryState::new(
                RecoveryDomain {
                    network: 3,
                    chain_id: CHAIN.into(),
                    genesis_digest: digest,
                    account_id: actor65.id,
                },
                recovery_config,
                actor65.identity(),
                0,
            )
            .unwrap(),
        },
    );
    book.validate().unwrap();
    let ordinary = fixture.config.ordinary.as_mut().unwrap();
    ordinary.origins.remove(&old_id).unwrap();
    ordinary
        .origins
        .insert(hex::encode(actor65.id), actor65.identity());
    ordinary
        .fee_profile
        .limits
        .allowed_algorithms
        .insert("mldsa65".into());
    ordinary
        .fee_profile
        .signature_costs
        .insert("mldsa65".into(), 3);
    let keys = BTreeMap::from([
        ("active", ClientKey::from_operational(&fixture.active)),
        ("payer", ClientKey::from_operational(&fixture.payer)),
        ("secondary", actor65),
    ]);
    (fixture, keys)
}

#[cfg(unix)]
fn private_empty_directory(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute(),
        "Fixture export requires an absolute directory"
    );
    let metadata =
        fs::symlink_metadata(path).context("Fixture output directory must already exist")?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Fixture output must be a real directory"
    );
    ensure!(
        fs::read_dir(path)?.next().is_none(),
        "Fixture output directory must be empty"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    let canonical = fs::canonicalize(path)?;
    ensure!(
        fs::metadata(&canonical)?.permissions().mode() & 0o777 == 0o700,
        "Fixture directory privacy check failed"
    );
    ensure!(
        fs::read_dir(&canonical)?.next().is_none(),
        "Fixture output directory changed"
    );
    Ok(canonical)
}
#[cfg(unix)]
fn private_file(directory: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    ensure!(
        !name.contains('/') && !name.contains('\\'),
        "Fixture filename must be local"
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(name))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    ensure!(
        file.metadata()?.permissions().mode() & 0o777 == 0o600,
        "Fixture file privacy check failed"
    );
    Ok(())
}

#[test]
fn export_disposable_ordinary_client_fixture() {
    let (fixture, keys) = fixture_with_operational_keys();
    // Round-trip exact client transport bytes before validating the full node.
    // Compact configuration is required because public keys occur twice.
    let config_bytes = serde_json::to_vec(&fixture.config).unwrap();
    assert!(
        config_bytes.len() <= 65_536,
        "Client fixture exceeds stdio config bound"
    );
    let config: ConsensusConfig = serde_json::from_slice(&config_bytes).unwrap();
    config.validate().unwrap();
    let validation = tempfile::tempdir().unwrap();
    let database = validation.path().join("validation-db");
    let mut app =
        ConsensusApplication::open(&database, config.clone(), fixture.genesis.clone()).unwrap();
    let initialized = app
        .init_chain(CHAIN, 1, &fixture.genesis, &config.validators)
        .unwrap();
    assert_eq!(initialized.height, 0);
    let profile = app.query_ordinary_profile().unwrap();
    let accounts: BTreeMap<_, _> = keys
        .iter()
        .map(|(role, key)| {
            (
                *role,
                app.query_ordinary_account(&hex::encode(key.id)).unwrap(),
            )
        })
        .collect();
    for account in accounts.values() {
        assert!(!account.is_null());
    }

    drop(app);
    let reopened =
        ConsensusApplication::open(&database, config.clone(), fixture.genesis.clone()).unwrap();
    assert_eq!(reopened.info().unwrap().app_hash, initialized.app_hash);
    assert_eq!(reopened.query_ordinary_profile().unwrap(), profile);
    drop(reopened);

    // This remains an active test when export is disabled. It validates all
    // node genesis, authority, origin, proof-role, custody and restart checks.
    let Some(directory) = std::env::var_os(EXPORT_DIRECTORY) else {
        return;
    };
    #[cfg(not(unix))]
    panic!("Fixture export requires Unix private file permissions");
    #[cfg(unix)]
    {
        let directory = private_empty_directory(Path::new(&directory)).unwrap();
        let lifecycle = config.lifecycle.as_ref().unwrap();
        let init = json!({"method":"init_chain","payload":{
            "chain_id":CHAIN,"initial_height":1,"app_state_bytes":B64.encode(&fixture.genesis),
            "validators":config.validators,"evidence_max_age_blocks":lifecycle.evidence_max_age_blocks,
            "evidence_max_age_seconds":lifecycle.evidence_max_age_seconds,"evidence_max_age_nanos":0}});
        let identities: Vec<_> = keys
            .iter()
            .map(|(role, key)| {
                let mut value = key.public_view();
                value["role"] = json!(role);
                value
            })
            .collect();
        let secret_accounts:Vec<_>=keys.iter().map(|(role,key)|json!({"role":role,"account_id":hex::encode(key.id),"algorithm":key.algorithm,"public_key":key.public,"private_key":key.private})).collect();
        private_file(&directory, "config.json", &config_bytes).unwrap();
        private_file(&directory, "genesis.json", &fixture.genesis).unwrap();
        private_file(
            &directory,
            "profile.json",
            &serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        private_file(
            &directory,
            "public-identities.json",
            &serde_json::to_vec(&json!({"accounts":identities})).unwrap(),
        )
        .unwrap();
        private_file(
            &directory,
            "init-chain.json",
            &serde_json::to_vec(&init).unwrap(),
        )
        .unwrap();
        private_file(
            &directory,
            "secret-fixtures.json",
            &serde_json::to_vec(&json!({"accounts":secret_accounts})).unwrap(),
        )
        .unwrap();
        for (role, key) in &keys {
            let state = &config.recovery.as_ref().unwrap().accounts[&hex::encode(key.id)].recovery;
            let context = json!({"domain":state.domain,"current_key":state.active_key,
                "authorization_generation":"0","spending_nonce":"0","committed":profile["context"],
                "profile_digest":ordinary_fees::profile_digest(&config.ordinary.as_ref().unwrap().fee_profile).unwrap(),"protected":false});
            private_file(
                &directory,
                &format!("account-{role}.json"),
                &serde_json::to_vec(&accounts[role]).unwrap(),
            )
            .unwrap();
            private_file(
                &directory,
                &format!("context-{role}.json"),
                &serde_json::to_vec(&context).unwrap(),
            )
            .unwrap();
            private_file(
                &directory,
                &format!("key-{role}.json"),
                &serde_json::to_vec(&key.cli_key()).unwrap(),
            )
            .unwrap();
        }
        let actions = vec![OrdinaryAction::Send {
            recipient: fixture.payer.id(),
            denomination: Denomination::Udrt,
            amount: 7,
        }];
        private_file(
            &directory,
            "actions.json",
            &serde_json::to_vec(&actions).unwrap(),
        )
        .unwrap();
        private_file(&directory,"manifest.json",&serde_json::to_vec(&json!({
            "version":1,"fixture_only":true,"network":"development","chain_id":CHAIN,
            "genesis_sha256":config.app_state_sha256,"initialized_app_hash":initialized.app_hash,
            "config_sha256":hex::encode(Sha256::digest(&config_bytes)),"initial_height":1,
            "algorithms":["mldsa65"],"funded_accounts":keys.len(),
            "validation":"ConsensusApplication::open + init_chain + reopen",
            "export_contains_disposable_private_keys":true
        })).unwrap()).unwrap();
        fs::File::open(&directory).unwrap().sync_all().unwrap();
    }
}
