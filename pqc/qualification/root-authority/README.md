# Root authority qualification candidate

Launch status: **NO GO**. Production integration: **absent**.

This isolated Go module copies the existing root verifier and tests without changes.
The source is `dytallix-node/consensus/root-authorization`.
The copied module files retain their original hashes and dependency versions.
No existing node, wallet, manifest, lock file, or build command imports this directory.
The existing Rust package does not build this Go module.

`key_validation.go` adds three local checks:

- `ValidatePublicKey` checks the fixed-profile public key encoding.
- `CheckKeyPair` compares the embedded public key with an independently trusted public key. It signs and verifies a separate key-validation challenge. This rejects inconsistent secret seeds and public roots that a length check alone cannot detect.
- `SignForPolicy` checks chain, action, sequence, artifact digest, and validity height before signing. It checks the key pair and verifies the resulting signature before returning it.

SLH-DSA public keys consist of byte strings. A 64-byte value can pass the encoding check without a trusted source or a known private key. The function does not pretend otherwise.
Tests explicitly distinguish a structurally valid zero public key from an invalid zero private/public pair.
The local parameter set remains `SLH-DSA-SHAKE-256s`. Production parameter review remains open.

Tests use temporary keys in process memory. Tests write no private keys.
Key erasure, entropy failure, hardware custody, side-channel behavior, and independent cryptographic validation remain unqualified.
The new checked signing path adds a separate sign/verify operation. Production latency and admission limits need measurement.
The original `Sign` API remains available in this isolated copy. Production integration must expose only the approved signing interface.

The accompanying Track 2 record contains exact commands, source hashes, test logs, custody procedures, recovery procedures, and integration blockers.
