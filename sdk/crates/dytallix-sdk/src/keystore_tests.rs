use super::*;

const PASS: &[u8] = b"correct horse battery staple";

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dytallix-keystore-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("keystore.json")
}
fn raw(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &serde_json::Value) {
    fs::write(path, value.to_string()).unwrap();
}

#[test]
fn private_keys_are_encrypted_and_the_file_is_owner_only() {
    let path = temp("encrypted");
    let keypair = DytallixKeypair::generate();
    let mut keystore = Keystore::create(path.clone(), PASS).unwrap();
    keystore.add_keypair(&keypair, "one").unwrap();
    keystore.save().unwrap();
    let file = raw(&path);
    assert_eq!(file["version"], 2);
    assert_eq!(file["cipher"], "aes-256-gcm");
    assert_eq!(file["kdf"]["algorithm"], "argon2id");
    assert_eq!(file["kdf"]["memory_kib"], KDF_MEMORY_KIB);
    assert_eq!(file["kdf"]["iterations"], KDF_ITERATIONS);
    assert!(file["entries"][0].get("private_key").is_none());
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains(&serde_json::to_string(keypair.private_key()).unwrap()));
    assert!(!text.contains(&B64.encode(keypair.private_key())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_locked_keystore_lists_but_needs_the_passphrase_for_keys() {
    let path = temp("unlock");
    let keypair = DytallixKeypair::generate();
    let mut keystore = Keystore::create(path.clone(), PASS).unwrap();
    keystore.add_keypair(&keypair, "one").unwrap();
    keystore.save().unwrap();

    let mut opened = Keystore::open(path.clone()).unwrap();
    assert_eq!((opened.version(), opened.is_unlocked()), (2, false));
    assert_eq!(opened.list()[0].name, "one");
    assert_eq!(opened.active().unwrap().public_key, keypair.public_key());
    assert!(matches!(
        opened.get_keypair("one"),
        Err(SdkError::KeystoreLocked)
    ));
    assert!(matches!(
        opened.unlock(b"wrong"),
        Err(SdkError::KeystorePassphrase)
    ));
    assert!(matches!(
        opened.unlock(b""),
        Err(SdkError::KeystorePassphrase)
    ));
    let without_unlock = opened.open_keypair("one", PASS).unwrap();
    assert_eq!(without_unlock.private_key(), keypair.private_key());
    assert!(!opened.is_unlocked());
    opened.unlock(PASS).unwrap();
    let restored = opened.get_keypair("one").unwrap();
    assert_eq!(restored.public_key(), keypair.public_key());
    assert_eq!(restored.private_key(), keypair.private_key());
    // A second key in the unlocked store uses its own nonce.
    opened
        .add_keypair(&DytallixKeypair::generate(), "two")
        .unwrap();
    opened.save().unwrap();
    let file = raw(&path);
    assert_ne!(file["entries"][0]["nonce"], file["entries"][1]["nonce"]);
    let mut again = Keystore::open(path.clone()).unwrap();
    again.unlock(PASS).unwrap();
    assert_eq!(again.list().len(), 2);
    again.get_keypair("two").unwrap();
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn edited_metadata_fails_authentication() {
    let path = temp("tamper");
    let mut keystore = Keystore::create(path.clone(), PASS).unwrap();
    keystore
        .add_keypair(&DytallixKeypair::generate(), "one")
        .unwrap();
    keystore
        .add_keypair(&DytallixKeypair::generate(), "two")
        .unwrap();
    keystore.save().unwrap();
    let original = raw(&path);
    // Swapping two entries' ciphertexts, or renaming one, is detected.
    for change in [
        Box::new(|v: &mut serde_json::Value| v["entries"][0]["name"] = "renamed".into())
            as Box<dyn Fn(&mut serde_json::Value)>,
        Box::new(|v| v["entries"][0]["created_at"] = 1.into()),
        Box::new(|v| {
            let other = v["entries"][1]["ciphertext"].clone();
            v["entries"][0]["ciphertext"] = other;
            let nonce = v["entries"][1]["nonce"].clone();
            v["entries"][0]["nonce"] = nonce;
        }),
    ] {
        let mut edited = original.clone();
        change(&mut edited);
        write(&path, &edited);
        let mut opened = Keystore::open(path.clone()).unwrap();
        opened.unlock(PASS).unwrap();
        let name = opened.list()[0].name.clone();
        assert!(matches!(
            opened.get_keypair(&name),
            Err(SdkError::KeystoreCorrupt(_))
        ));
    }
    // An altered passphrase check refuses the passphrase.
    let mut edited = original;
    edited["check"]["ciphertext"] = B64.encode([0u8; 53]).into();
    write(&path, &edited);
    assert!(matches!(
        Keystore::open(path.clone()).unwrap().unlock(PASS),
        Err(SdkError::KeystorePassphrase)
    ));
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_version_1_file_lists_but_its_keys_wait_for_migration() {
    let path = temp("migrate");
    let keypair = DytallixKeypair::generate();
    let entry = serde_json::json!({
        "name": "old",
        "address": DAddr::from_public_key(keypair.public_key()).unwrap(),
        "public_key": keypair.public_key(),
        "private_key": keypair.private_key(),
        "scheme": keypair.scheme(),
        "created_at": 7,
    });
    // A file written before interfaces v1 has no version: it is version 1.
    write(
        &path,
        &serde_json::json!({"active": "old", "entries": [entry]}),
    );
    let mut keystore = Keystore::open(path.clone()).unwrap();
    assert_eq!(keystore.version(), 1);
    assert_eq!(keystore.list()[0].name, "old");
    assert!(matches!(
        keystore.get_keypair("old"),
        Err(SdkError::KeystorePlaintext)
    ));
    assert!(matches!(
        keystore.add_keypair(&DytallixKeypair::generate(), "new"),
        Err(SdkError::KeystorePlaintext)
    ));
    assert!(matches!(
        keystore.unlock(PASS),
        Err(SdkError::KeystorePlaintext)
    ));
    keystore.migrate(PASS).unwrap();
    assert!(keystore.migrate(PASS).is_err());
    keystore.save().unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("private_key"));
    let mut migrated = Keystore::open(path.clone()).unwrap();
    assert_eq!(migrated.version(), 2);
    assert_eq!(migrated.list()[0].created_at, 7);
    migrated.unlock(PASS).unwrap();
    assert_eq!(
        migrated.get_keypair("old").unwrap().private_key(),
        keypair.private_key()
    );
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn unsupported_formats_and_parameters_are_refused() {
    let path = temp("formats");
    let mut keystore = Keystore::create(path.clone(), PASS).unwrap();
    keystore
        .add_keypair(&DytallixKeypair::generate(), "one")
        .unwrap();
    keystore.save().unwrap();
    let original = raw(&path);
    type Change = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<Change> = vec![
        Box::new(|v| v["version"] = 3.into()),
        Box::new(|v| v["cipher"] = "xchacha20poly1305".into()),
        Box::new(|v| v["kdf"]["algorithm"] = "scrypt".into()),
        Box::new(|v| v["kdf"]["memory_kib"] = 1024.into()),
        Box::new(|v| v["kdf"]["memory_kib"] = (8u64 * 1024 * 1024).into()),
        Box::new(|v| v["kdf"]["iterations"] = 1.into()),
        Box::new(|v| v["kdf"]["iterations"] = 1000.into()),
        Box::new(|v| v["kdf"]["parallelism"] = 0.into()),
        Box::new(|v| v["kdf"]["salt"] = "c2hvcnQ=".into()),
        Box::new(|v| v["entries"][0]["note"] = "extra".into()),
    ];
    for change in cases {
        let mut edited = original.clone();
        change(&mut edited);
        write(&path, &edited);
        assert!(
            matches!(
                Keystore::open(path.clone()),
                Err(SdkError::KeystoreCorrupt(_))
            ),
            "{edited}"
        );
    }
    // Two keystores with one passphrase use different salts.
    let other = Keystore::create(temp("salt"), PASS).unwrap();
    other.save().unwrap();
    assert_ne!(raw(&other.path)["kdf"]["salt"], original["kdf"]["salt"]);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
    fs::remove_dir_all(other.path.parent().unwrap()).unwrap();
}
