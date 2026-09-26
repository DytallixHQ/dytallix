# Approved PQC profile

Launch status: **NO GO**.

Post-quantum cryptography (PQC) replaces asymmetric cryptography that a
cryptographically relevant quantum computer can break. G35 applies to that
asymmetric cryptography inside the production trust boundary. G35 does not
prohibit symmetric encryption, message authentication codes, or cryptographic hashes.

| Role | Approved primitive | SDK state |
| --- | --- | --- |
| Operational signatures | FIPS 204 ML-DSA-65 | Default key generation, wallet keystore, and legacy transaction signing require exact ML-DSA-65. |
| P2P key establishment | FIPS 203 ML-KEM-768 | Node transport responsibility. This SDK does not implement P2P. |
| Root and exceptional authorization | FIPS 205 SLH-DSA | Separate node component. SDK root signing and CLI root verification are unavailable. |

The standalone ordinary-v2 development interface retains explicitly selected
ML-DSA-87. Its account-role profile must authorize the exact scheme. This
compatibility interface does not override the approved production ML-DSA-65
profile. It does not authorize mainnet.

The Rust variant `KeyScheme::SlhDsa`, `generate_slh_dsa`, and `verify_slhdsa`
remain legacy compatibility names. Their backend is
`pqcrypto-sphincsplus::sphincsshake192ssimple`. It implements legacy
SPHINCS+-SHAKE-192s-simple. It does not establish FIPS 205 conformance.
New serialization uses `LegacySphincsPlusShake192sSimple`. Existing serialized
`SlhDsa` values still decode. Existing key bytes do not change. Do not migrate
legacy keys by relabeling them. Equal key or signature sizes do not prove
algorithm identity or signature interoperability.

The CLI rejects `crypto keygen --scheme slh-dsa`. Its operational verifier
accepts exact ML-DSA-65 and returns a failing status for invalid signatures.
The verifier rejects 48-byte keys because that length does not identify FIPS 205.
Use the separate candidate root authorization component for local genesis,
upgrade, and emergency governance tests. Complete independent review before
production use. Require the expected chain, action, target digest,
validity bounds, and externally trusted root public key there.

The wallet checks the declared scheme, supplied key pair, and derived address
when it reloads a keystore. This check preserves existing valid ML-DSA-65 keys.
The keystore currently writes plaintext with no explicit file permission mode,
atomic replacement, or encryption. Wallet custody is not production qualified.
Raw private-only import remains a legacy API with the pinned backend's malformed
secret limitations. New applications should import an explicit public/private
pair with `from_keypair`.

SHA3-256 remains the legacy transaction digest. BLAKE3 remains in address
hashing and deterministic signing seed derivation. Retaining those functions
requires a reviewed hash security budget and compatible protocol definitions.
The SDK's deterministic seed derivation is an application construction. These
tests do not establish a FIPS-validated cryptographic module.

SDK HTTP uses `reqwest` with default native TLS. On this macOS build its chain
includes `native-tls`, `hyper-tls`, and `security-framework`. The SDK does not
restrict TLS to post-quantum asymmetric authentication or key establishment.
Default HTTPS therefore does not close G35 for a production RPC trust boundary.
Qualify a PQC-authenticated transport or remove reliance on that channel for
production authentication and integrity through a reviewed protocol.

## Acceptance criteria

- Reject non-ML-DSA-65 keys in the production operational signing path.
- Preserve valid wallet keys through save and reload.
- Reject scheme, address, and public/private mismatches during wallet reload.
- Reject legacy SPHINCS+ as FIPS 205 evidence.
- Preserve canonical ordinary transaction bytes and exact algorithm identifiers.
- Distinguish submission admission from committed receipts.
- Verify receipt identity, nonce, charge, release, and failure semantics.
- Qualify restart and SDK-to-node behavior with the current source-built node.
- Qualify source packaging separately, with a recorded artifact digest.
- Review RPC transport, browser wallet integration, root authorization, and
  P2P transport before changing NO GO.

Local SDK and CLI tests do not qualify a browser wallet, deployed network,
consensus engine, validated cryptographic module, or mainnet launch.
