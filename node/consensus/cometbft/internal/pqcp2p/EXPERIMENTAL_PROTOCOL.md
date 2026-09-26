# Local experimental PQC transport, wire version 1

This package is for explicit local integration. It has not received independent protocol review. `RequireProductionTransport` always returns `ErrProductionBlocked`. Component and integration tests do not change this gate.

## Identity and peer selection

`ImportIdentity` accepts the 4032-byte packed CIRCL ML-DSA-65 private key used by the older loopback profile. It does not accept a seed. Import checks the eta encoding, canonical packing, and signing capability. Verification uses a freshly parsed public key. This interface accepts trusted local key files only. It does not certify complete packed-key consistency. A small inconsistent `t0` change can still produce a valid signature. Full consistency remains an open qualification requirement for that profile.

`ImportSeedIdentity` derives an identity from an exact 32-byte seed and requires the complete expected 1952-byte public key. The separate `dytallix-pqc-loopback-seed-v1` engine profile reads `config/pqc_peer_seed.bin` from an existing private home. It rejects symlinks, hard links, other owners, public file modes, incorrect lengths, and a coexisting `config/node_key.json`. The engine derives its transport identity and Comet node key from the same seed. The fixture generator creates this file once for disposable local nodes. Startup does not generate or replace it. This profile still requires loopback listeners and explicit full-key peer pins. It does not define production custody or authorize remote transport.

`dytallix-pqc-private-seed-v1` is a separate staging profile. It requires one explicit private IP endpoint per host, distinct host IPs for all pinned peers, complete public-key pins, bounded peer counts, and an explicit network string. It keeps RPC on loopback and the application connection on a local Unix socket. It rejects unpinned inbound source IPs before the handshake. After the handshake, it binds the authenticated full peer key to the configured source IP. It checks the live outbound endpoint against the pin. It has no public-IP or legacy-transport fallback. The selected strict build uses both `dytallix_pqc_only` and `dytallix_pqc_ipc` tags. The fixture generator creates all private keys in one output tree and is suitable only for disposable staging tests. Current tests use simulated connection addresses; they do not establish separate-host behavior or operator custody. The command still rejects `--production`.

`dytallix-pqc-production-candidate-v1` has offline transport-policy and complete-file validators plus a candidate peer-upgrade branch. It requires the strict PQC-only IPC build, one explicit canonical IP endpoint per peer host, distinct host IPs, full ML-DSA-65 public-key pins, a matching persistent-peer set, an exact chain ID, bounded admission, loopback RPC input, local Unix ABCI and no browser origin. It accepts private or public global-unicast IP literals because the production topology and address list remain unset. The file validator reads existing private seed, genesis, transport, validator and state files without starting a service. `ValidateProductionCandidateBinding` also compares the exact config, genesis, and transport bytes parsed by that loader with a separately approved chain ID and SHA-256 digests. The caller must verify the approval record. The candidate peer branch binds the live source IP to the authenticated full key. The ordinary engine loader rejects this profile. `start --candidate-staging` can enter it only when the chain ID starts with `e01-candidate-` and does not contain `mainnet` or `production`. `--production` still returns `ErrProductionBlocked`. Disposable-file and simulated-connection checks do not approve the handshake protocol, custody, host firewall or final executable.

`Upgrade(raw, network, local, pins, remotePin, timeout)` consumes the connection. Failure closes the connection. Pins contain 1 to 64 distinct, complete 1952-byte ML-DSA-65 peer public keys. The list must not contain the local public key. The caller obtains pins from authenticated configuration.

A non-nil `remotePin` selects the initiator and must match one complete configured pin. A nil `remotePin` selects the responder. The responder accepts only a complete initiator key from its pin list. The exact network string binds the session. Its length is 1 to 64 bytes. There is no negotiation, downgrade, or plaintext fallback.

The caller must separately limit concurrent handshake attempts. A responder signs its offer before it receives the initiator's signature. A pin match alone does not prove that the sender owns that key.

## Handshake framing

The header has eight bytes: `DYPH`, version `1`, message type, and a two-byte big-endian payload length. The parser checks the exact message type and its allowed length before allocating the payload.

| Type | Payload |
| --- | --- |
| 1 | Network length (one byte), network, nonce (32 bytes), initiator public key (1952 bytes), responder public key (1952 bytes) |
| 2 | ML-KEM-768 public key (1184 bytes), responder ML-DSA-65 signature (3309 bytes) |
| 3 | ML-KEM-768 ciphertext (1088 bytes), initiator ML-DSA-65 signature (3309 bytes) |
| 4 | Server key confirmation (32 bytes) |
| 5 | Client key confirmation (32 bytes) |

The existing establishment transcript binds the fixed suite, network, nonce, both role keys, ML-KEM key, ciphertext, and signatures. HKDF-SHA256 derives separate directional traffic keys and confirmation keys. Both sides check key confirmation before returning a connection. The initiator sends its final confirmation before returning.

One absolute I/O deadline covers the handshake. The timeout must be positive and at most one minute. Successful upgrade clears this deadline. The caller then sets application deadlines. Cryptographic operations have bounded inputs; the I/O deadline does not interrupt a cryptographic operation in progress.

## Authenticated records

Each record has a 16-byte header: `DYPR`, version `1`, reserved zero byte, sequence (eight bytes, big-endian), and plaintext length (two bytes, big-endian). ChaCha20-Poly1305 authenticates the complete header as associated data. The ciphertext has the stated plaintext length plus the 16-byte authentication tag.

Each direction starts at sequence zero and uses its own traffic key. The 12-byte nonce contains four zero bytes followed by the eight-byte big-endian sequence. The receiver requires the next exact sequence. Record plaintext has 1 to 16384 bytes. Empty application writes produce no record.

Each direction permits at most 1048576 records, or at most 16 GiB of plaintext. The next record operation after that limit closes the connection. The code never wraps the counter or resets a key within a connection. A new handshake with fresh randomness and a fresh ephemeral ML-KEM key is required to continue.

Reads authenticate the complete record before exposing plaintext. Writes split larger application buffers into bounded records. A partial frame, invalid tag, invalid header, unexpected sequence, deadline error, or record-limit error closes the stream. No operation can restart that stream. `Close` also prevents later reads from returning cached plaintext. One reader and one writer can operate concurrently. `Close` does not wait for either I/O lock.

A clean transport EOF does not prove application-message completion. The application protocol must check its own complete message framing. Record lengths are visible. This version has no padding or authenticated close message. Go does not guarantee erasure of private-key or cipher objects.
