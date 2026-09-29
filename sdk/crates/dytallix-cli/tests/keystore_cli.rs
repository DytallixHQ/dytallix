//! The CLI's encrypted keystore (E04 gap 16): creation under a passphrase
//! file, listing without one, an owner-only export, and the migration of a
//! version 1 (plaintext) keystore.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const ENV: &str = "DYTALLIX_KEYSTORE_PASSPHRASE_FILE";

fn home(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dytallix-cli-keystore-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn passphrase_file(home: &Path, name: &str, text: &str) -> PathBuf {
    let path = home.join(name);
    std::fs::write(&path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}
/// Every run names a passphrase file, so none prompts on a terminal.
fn run(home: &Path, passphrase: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dytallix"))
        .args(args)
        .env("HOME", home)
        .env(ENV, passphrase)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn keystore(home: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(home.join(".dytallix/keystore.json")).unwrap()).unwrap()
}

#[test]
fn wallets_are_encrypted_and_listing_needs_no_passphrase() {
    let home = home("encrypted");
    let right = passphrase_file(&home, "right", "cli passphrase\n");
    let wrong = passphrase_file(&home, "wrong", "other passphrase\n");
    let missing = home.join("missing");

    let created = run(&home, &right, &["wallet", "create", "--name", "one"]);
    assert!(created.status.success(), "{}", text(&created));
    let file = keystore(&home);
    assert_eq!(file["version"], 2);
    assert_eq!(file["cipher"], "aes-256-gcm");
    assert!(file["entries"][0].get("private_key").is_none());

    for args in [&["wallet", "list"][..], &["wallet", "info"]] {
        let output = run(&home, &missing, args);
        assert!(output.status.success(), "{args:?}: {}", text(&output));
        assert!(text(&output).contains("one"));
    }
    assert!(text(&run(&home, &missing, &["wallet", "info"])).contains("encrypted"));

    let export = home.join("export.hex");
    let export_arg = export.to_str().unwrap();
    assert!(!run(
        &home,
        &missing,
        &["wallet", "export", "--output", export_arg]
    )
    .status
    .success());
    let refused = run(&home, &wrong, &["wallet", "export", "--output", export_arg]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("Wrong keystore passphrase"),
        "{}",
        text(&refused)
    );
    assert!(!export.exists());
    let exported = run(&home, &right, &["wallet", "export", "--output", export_arg]);
    assert!(exported.status.success(), "{}", text(&exported));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&export).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // An export never overwrites a file.
    assert!(
        !run(&home, &right, &["wallet", "export", "--output", export_arg])
            .status
            .success()
    );
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_version_1_keystore_is_migrated_before_its_keys_are_used() {
    let home = home("migrate");
    let right = passphrase_file(&home, "right", "migration passphrase");
    let keypair = dytallix_core::keypair::DytallixKeypair::generate();
    let entry = serde_json::json!({
        "name": "old",
        "address": dytallix_core::address::DAddr::from_public_key(keypair.public_key()).unwrap(),
        "public_key": keypair.public_key(),
        "private_key": keypair.private_key(),
        "scheme": keypair.scheme(),
        "created_at": 1,
    });
    std::fs::create_dir_all(home.join(".dytallix")).unwrap();
    std::fs::write(
        home.join(".dytallix/keystore.json"),
        serde_json::json!({"version": 1, "active": "old", "entries": [entry]}).to_string(),
    )
    .unwrap();
    let export = home.join("export.hex");
    let export_arg = export.to_str().unwrap();

    let listed = run(&home, &right, &["wallet", "list"]);
    assert!(
        listed.status.success() && text(&listed).contains("old"),
        "{}",
        text(&listed)
    );
    let refused = run(&home, &right, &["wallet", "export", "--output", export_arg]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("dytallix wallet migrate"),
        "{}",
        text(&refused)
    );
    let refused = run(&home, &right, &["wallet", "create", "--name", "new"]);
    assert!(!refused.status.success() && text(&refused).contains("dytallix wallet migrate"));

    let migrated = run(&home, &right, &["wallet", "migrate"]);
    assert!(migrated.status.success(), "{}", text(&migrated));
    assert_eq!(keystore(&home)["version"], 2);
    assert!(
        !String::from_utf8(std::fs::read(home.join(".dytallix/keystore.json")).unwrap())
            .unwrap()
            .contains("private_key")
    );
    assert!(!run(&home, &right, &["wallet", "migrate"]).status.success());
    let exported = run(&home, &right, &["wallet", "export", "--output", export_arg]);
    assert!(exported.status.success(), "{}", text(&exported));
    let hex: String = keypair
        .private_key()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(std::fs::read_to_string(&export).unwrap(), hex);
    std::fs::remove_dir_all(&home).unwrap();
}
