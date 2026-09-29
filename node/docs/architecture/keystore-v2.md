# Encrypted keystore (E04 gap 16)

Engineering task E04, gap 16 of the [E04.1 triage](../mainnet/e04-requirement-triage.md).
Gap 9 found it: the CLI keystore (`~/.dytallix/keystore.json`, version 1)
held private keys in plaintext.

## Decisions (P01, 28 September 2026)

1. **Key derivation: Argon2id** (RFC 9106), version 0x13, 32-byte output.
   - Parameters and a 16-byte random salt are stored in the file.
   - New files use 64 MiB, 3 passes and 1 lane.
   - A file below those values, or above the ceilings (4 GiB, 64 passes,
     16 lanes), is refused.
2. **Cipher: AES-256-GCM** (NIST SP 800-38D). Each entry has a random
   96-bit nonce. XChaCha20-Poly1305 was the first choice; it was replaced
   by the NIST-approved cipher.
3. **Scope: private keys encrypted, metadata bound.**
   - Names, addresses, public keys, schemes and creation times stay
     readable, so listing needs no passphrase.
   - Each ciphertext takes the entry's metadata as associated data, so
     editing a field, or swapping two entries' ciphertexts, fails
     authentication.
   - A fixed passphrase check, sealed under the same key, refuses a wrong
     passphrase before any key is used.
4. **Passphrases and migration.**
   - A passphrase is typed on the terminal with echo off, or read from the
     owner-only file named by `DYTALLIX_KEYSTORE_PASSPHRASE_FILE`, with one
     trailing line ending dropped. It is never taken from an environment
     variable's value or an argument. Empty ones and ones over 1 KiB are
     refused.
   - A version 1 file still opens and lists. Every command that needs a
     private key refuses it and names `dytallix wallet migrate`.
   - The migrate command asks for a new passphrase twice. It then replaces
     the file atomically (owner-only, through a temporary file and a
     rename) and keeps no plaintext copy.

## Quantum resistance

Both primitives are symmetric. The quantum threat to classical
cryptography is to public-key algorithms (Shor's algorithm); none is
involved here.
- Against quantum search (Grover's algorithm), the 256-bit AES key keeps
  about 128-bit security.
- The passphrase search is bounded by Argon2id's memory cost and the
  passphrase itself.

The FIPS 203 (ML-KEM) and FIPS 204 (ML-DSA) algorithms rest on symmetric
primitives in the same way.

## Format (version 2)

```json
{"version": 2, "cipher": "aes-256-gcm",
 "kdf": {"algorithm": "argon2id", "version": 19, "memory_kib": 65536,
         "iterations": 3, "parallelism": 1, "salt": "base64"},
 "check": {"nonce": "base64", "ciphertext": "base64"},
 "active": "name",
 "entries": [{"name": "...", "address": "...", "public_key": [...],
              "scheme": "...", "created_at": 0,
              "nonce": "base64", "ciphertext": "base64"}]}
```

The associated data are `dytallix-keystore-v2\0` followed by the entry's
metadata as JSON, or `dytallix-keystore-v2\0check` for the check. Unknown
fields, versions and ciphers are refused.

## Code

- **SDK** (`dytallix-sdk::keystore`):
  - `Keystore::create(path, passphrase)`, `open` (locked), `unlock`,
    `open_keypair` (without unlocking), `get_keypair` (unlocked),
    `add_keypair` (unlocked version 2 only) and `migrate`;
  - `version` and `is_unlocked`;
  - errors `KeystoreLocked`, `KeystorePlaintext` and `KeystorePassphrase`;
  - `KeystoreEntry` carries public metadata only.
- **CLI:**
  - every command that uses a private key reads the passphrase (the
    `commands::passphrase` module). The `ordinary` signing path and the
    active-wallet lookup now go through the keystore instead of reading
    the file;
  - `wallet info` and `wallet list` need no passphrase;
  - `wallet export` writes a new owner-only file;
  - `wallet migrate` is new.
- **Build:** the SDK workspace builds `argon2` and `blake2` optimized in
  development and test profiles.
