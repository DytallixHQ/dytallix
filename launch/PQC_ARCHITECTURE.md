# Dytallix PQC architecture

Status: algorithms approved for local implementation. Production qualification is open. Mainnet remains **NO GO**.

The [profile](decision-register/pqc-profile/PROFILE.json) and [G35 specification](decision-register/pqc-profile/GATE_SPEC.json) supersede earlier broad prohibitions on all classical cryptography. Preserve the [prior architecture](decision-register/pqc-profile/evidence/before/PQC_ARCHITECTURE.md) as historical evidence.

| Role | Selected algorithm | Implementation boundary |
|---|---|---|
| P2P key establishment | FIPS 203 ML-KEM-768 | Authenticated session component under development. Active upstream SecretConnection still uses X25519. Production integration remains blocked. |
| Peer identity | FIPS 204 ML-DSA-65 | Separate full public-key trust pins. Upstream Ed25519 identities and truncated peer IDs cannot establish the new trust boundary. |
| Validators, votes and proposals | FIPS 204 ML-DSA-65 | Current CometBFT validator support exists. Registration, codec/default restrictions, remote signer and custody qualification remain required. |
| Transactions, wallets and operational authorization | FIPS 204 ML-DSA-65 | Explicit parameter identifiers and key sizes. Preserve old keys and account identities. Do not reinterpret old signatures. |
| Genesis, upgrades and exceptional authorization | FIPS 205 SLH-DSA | Separate root keys and policy. Local component selects SHAKE-256s pending production review. No automatic execution or mainnet approval. |
| Symmetric encryption, derivation and hashes | Existing appropriate primitives | Review each use, key/output length, transcript binding, nonce policy and security requirement. |

ML-KEM establishes a shared secret. It does not authenticate a peer. Peer authentication must bind both identities, the chain, protocol version, roles and ephemeral key material to one transcript. Direction-specific keys and explicit key confirmation must reject mismatched sessions. The production protocol also needs framing, bounded allocation, timeouts, replay handling, rekeying, disconnect handling and secret erasure. The isolated component is not a reviewed network protocol.

The production trust boundary includes consensus, P2P, account and wallet authorization, recovery, governance, trusted RPC/proof paths, root authorization, custody and management channels required to control these systems. An external service cannot escape this boundary merely because it uses TLS, SSH or a browser. Classify its actual role and verify isolation.

Classical code can remain in separate development compatibility artifacts. A dependency name does not prove runtime use. An absent symbol does not prove isolation. The release must exclude or demonstrably isolate prohibited asymmetric implementations and fallback paths. The current upstream P2P path remains prohibited for production.

Keep ML-DSA-65, ML-DSA-87, legacy Dilithium and legacy SPHINCS+ distinct. Existing state needs an explicit migration. Stable account identifiers must not change when authorized keys rotate. Operational keys must not acquire root authority. Root signatures require a separately configured trusted key, chain, action, sequence and artifact commitment. Persistent replay policy and execution integration remain mandatory.

ChaCha20-Poly1305, HKDF/HMAC-SHA-256 and existing hashes are not prohibited merely because they are symmetric or hashing primitives. Their use still needs review. A category-3 KEM does not establish category-3 security for a complete system. Truncated hashes and hash collision requirements need separate analysis. Preserve consensus hash behavior until a reviewed migration exists.

The official standards are [FIPS 203](https://csrc.nist.gov/pubs/fips/203/final), [FIPS 204](https://csrc.nist.gov/pubs/fips/204/final) and [FIPS 205](https://csrc.nist.gov/pubs/fips/205/final). Check applicable errata during backend qualification. Algorithm use is not FIPS 140 module validation. No current result proves side-channel resistance, production custody or a reviewed release.

Required closure evidence includes known-answer and cross-implementation tests, all-path integration, exact source and artifact hashes, dependency/provider reachability, custody drills, and independent cryptography and protocol review. Seven full release-candidate simulations and all other launch gates remain open.
