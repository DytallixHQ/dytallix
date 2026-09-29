//! Keystore passphrases (E04 gap 16, P01 28 September 2026): typed on the
//! terminal without echo, or read from the owner-only file named by
//! `DYTALLIX_KEYSTORE_PASSPHRASE_FILE` for scripts. A passphrase is never
//! taken from an environment variable's value or a command-line argument.

use std::path::Path;

use anyhow::{anyhow, ensure, Context, Result};
use zeroize::Zeroizing;

/// The environment variable naming a passphrase file.
pub(crate) const FILE_ENV: &str = "DYTALLIX_KEYSTORE_PASSPHRASE_FILE";
const MAX_BYTES: usize = 1024;

/// The passphrase of an existing keystore.
pub(crate) fn existing() -> Result<Zeroizing<Vec<u8>>> {
    match std::env::var_os(FILE_ENV) {
        Some(path) => from_file(Path::new(&path)),
        None => prompt("Keystore passphrase: "),
    }
}

/// A new passphrase, typed twice on a terminal.
pub(crate) fn new() -> Result<Zeroizing<Vec<u8>>> {
    if let Some(path) = std::env::var_os(FILE_ENV) {
        return from_file(Path::new(&path));
    }
    let first = prompt("New keystore passphrase: ")?;
    let second = prompt("Repeat the passphrase: ")?;
    ensure!(first == second, "The passphrases differ");
    Ok(first)
}

fn prompt(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    let typed = Zeroizing::new(
        rpassword::prompt_password(text).context("Cannot read a passphrase from the terminal")?,
    );
    checked(Zeroizing::new(typed.as_bytes().to_vec()))
}

fn from_file(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Cannot read {FILE_ENV} ({})", path.display()))?;
        ensure!(
            metadata.is_file() && metadata.mode() & 0o077 == 0,
            "{FILE_ENV} must name a file readable by its owner only"
        );
    }
    let mut bytes = Zeroizing::new(
        std::fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?,
    );
    // One trailing line ending is the file's, not the passphrase's.
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    checked(bytes)
}

fn checked(bytes: Zeroizing<Vec<u8>>) -> Result<Zeroizing<Vec<u8>>> {
    if bytes.is_empty() {
        return Err(anyhow!("The passphrase is empty"));
    }
    ensure!(
        bytes.len() <= MAX_BYTES,
        "The passphrase exceeds {MAX_BYTES} bytes"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passphrase_file_is_owner_only_and_loses_one_line_ending() {
        let dir = std::env::temp_dir().join(format!("dytallix-passphrase-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("passphrase");
        std::fs::write(&path, b"secret words\r\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(from_file(&path).is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(from_file(&path).unwrap().as_slice(), b"secret words");
        std::fs::write(&path, b"\n").unwrap();
        assert!(from_file(&path).is_err());
        std::fs::write(&path, vec![b'a'; MAX_BYTES + 1]).unwrap();
        assert!(from_file(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
