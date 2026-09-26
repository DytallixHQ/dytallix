# Verification-only root helper

This command verifies one canonical public request against separately supplied trusted policy. It cannot sign, deploy, change chain state, consume replay sequences, or approve production.

The selected normal helper requires `--execution-profile linux-immutable-observed-helper-v1`, `--profile SLH-DSA-SHAKE-256s`, `--policy-json`, and `--max-input-bytes`. A missing or different execution profile fails before readiness. There is no implicit legacy transport fallback. The policy contains public authority and expected chain, action, sequence, validity context, and artifact digest. The caller must supply this policy independently of request fields. The request bound must be positive and fit an unsigned 32-bit length.

The observed protocol uses these steps:

1. The helper validates its arguments and canonical policy. It writes `DYTALLIX-ROOT-READY-v1\n` to stdout before it reads request bytes.
2. The caller observes the actual owned helper executable and mappings. It then writes a four-byte unsigned big-endian request length and the exact canonical request bytes to stdin. It keeps stdin open.
3. The helper writes one result frame: a one-byte status, a four-byte unsigned big-endian payload length, and the payload. Status `0` requires 1–4096 bytes of unchanged canonical `VerificationResult` JSON. Status `2` requires an empty payload and reports verification rejection. Other statuses are invalid.
4. The helper waits while the caller observes the same owned helper again. The caller then writes `DYTALLIX-ROOT-ACK-v1\n` and closes stdin. The helper requires both the exact token and EOF. Extra bytes, a missing ACK, or early EOF are infrastructure failures.
5. A verified result completes with natural exit `0` and empty stderr. A rejected result completes with exit `2` and exactly `root verification rejected\n` on stderr. Other errors exit `1`. The caller accepts neither result until it checks exit status and completes owned cleanup.

The tokens shown above contain one newline byte, not a backslash and letter. READY and ACK are local synchronization messages. They are not signatures or attestations. The helper blocks at these points; the caller must enforce one total deadline and all input/output limits. A handshake, observation, timeout, or cleanup failure must not become a deterministic authorization rejection.

Framing does not change canonical `VerificationRequest` or `VerificationResult` JSON. Byte slices use standard base64. The result binds the exact request SHA-256 and artifact SHA-512, chain, action, and sequence. `ProductionQualified` remains false. The helper does not own replay state. The Rust consumers retain their existing atomic state and receipt rules.

The Rust observed launcher must execute the pinned static ELF from an immutable root-owned read-only path. It must verify the owned process identity and allowed mappings before request release and after result delivery. The Go protocol alone does not establish those conditions. Bootstrap execution must use the original independent root policy. A candidate catalog cannot select the helper that verifies that same catalog. Live control verification may use a catalog only after independent authority verification and catalog acceptance. Scratch must not contain an executable helper copy in the observed profile.

Root configuration still binds actual engine genesis and release-manifest files to the signed bundle. Distribute the exact same signed root request bytes to every validator and retain them on restart. Do not re-sign or replace a request on an initialized database. This protocol change does not alter the root bundle or persisted root receipt format. A new helper binary requires new artifact pins and the corresponding authorized catalog and release evidence.

The test-only `TestExportDevelopmentGenesis` exporter remains separate from this release command. It requires explicit fixture inputs and keeps its temporary private key in memory. Do not distribute that test signer as a release helper. Custody, production root actions, independent review, and G35 acceptance remain separate work. The two observation points do not prove continuous enforcement.
