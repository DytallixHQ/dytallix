# Client channel (E04 gap 19)

Engineering task E04, gap 19 of the [E04.1 triage](../mainnet/e04-requirement-triage.md).
P01 decided on 28 September 2026 that no classical public-key cryptography
may appear anywhere in the stack. Two client paths still used it:
- the SDK and CLI reached remote nodes over HTTPS, through reqwest's platform
  TLS (OpenSSL, Security.framework or SChannel);
- the public gateway contract ([RPC controls v1](rpc-controls-v1.md))
  terminated TLS 1.3 with a certificate.

Both rely on classical key exchange and certificate signatures.

## Decisions (P01, 29 September 2026)

1. **Remote access: a post-quantum channel.**
   - ML-KEM-768 key exchange.
   - The endpoint signs with an ML-DSA-65 key that the client pins in full.
     There is no trust on first use.
   - Clients are anonymous, and records are encrypted.
   - Responses that carry state proofs are still verified against the
     pinned chain.
   - Every SDK and CLI feature drops TLS.
2. **Browsers: a local companion.**
   - `dytallix gateway` runs on the user's machine and serves the browser
     wallet on `http://127.0.0.1`, which browsers treat as a secure
     context.
   - It forwards requests over the channel.
   - There is no public HTTPS endpoint.
3. **Records: AES-256-GCM,** in the client channel and in the peer transport.
   The peer transport moves from ChaCha20-Poly1305 to AES-256-GCM as its wire
   version 2.
4. **Scope.**
   - The legacy testnet commands (the `legacy-network` and `network`
     features) leave the mainnet SDK and CLI.
   - The contracts toolkit drops its CosmWasm bridge.
   - Not selected: moving the testnet faucet, and replacing SSH host access.

Why not the alternatives:
- **Hybrid TLS** (X25519MLKEM768) still authenticates with classical
  certificates.
- **Plain HTTP with proofs only** leaves status, CheckTx and broadcast
  results forgeable, and exposes every query.
- **Same machine only** would make every user run a node.

## Protocol (version 1)

There are two roles:
- **the client**, which initiates and stays anonymous;
- **the endpoint**, which holds an ML-DSA-65 identity key.

The endpoint key is a role key of its own, derived from a 32-byte seed by
FIPS 204 key generation. It is distinct from validator and peer keys. The
client pins its full 1,952-byte public key. The network string is the chain
ID, 1 to 64 bytes.

The suite is fixed: `dytallix-client-channel-v1/mlkem768/mldsa65/hkdfsha256/aes256gcm`.
There is no negotiation, downgrade or plaintext fallback.

### Handshake

Each message has an eight-byte header: `DYCH`, version `1`, the message
type, and a two-byte big-endian payload length. The reader checks the type
and the exact allowed length before it allocates the payload.

| Type | From | Payload | Bytes |
| --- | --- | --- | --- |
| 1 Hello | client | network length (1), network, nonce (32), pinned endpoint key (1952), ML-KEM-768 encapsulation key (1184) | 3170–3233 |
| 2 Offer | endpoint | ML-KEM-768 ciphertext (1088), ML-DSA-65 signature (3309), endpoint confirmation (32) | 4429 |
| 3 Finish | client | client confirmation (32) | 32 |

1. **Hello.**
   - The client generates a fresh ML-KEM-768 key pair for each connection.
   - The endpoint refuses a hello that names another network or another key.
   - It also refuses an encapsulation key with a coefficient at or above q.
     That is the FIPS 203 input check.
2. **Offer.**
   - The endpoint encapsulates to the client's key.
   - It signs `encode(suite, network, nonce, endpoint key, encapsulation key, ciphertext)`
     with the context `suite + "/endpoint-offer"`. `encode` joins its parts,
     each prefixed with its four-byte big-endian length, as the peer
     transport does.
3. **Keys.**
   - The transcript is `SHA-256(encode(signed message, signature))`.
   - HKDF-SHA-256 takes the ML-KEM shared secret as input, with the
     transcript as its salt and `suite + "/traffic-and-confirmation"` as its
     info. It yields 128 bytes:
     - the client-to-endpoint key;
     - the endpoint-to-client key;
     - the endpoint confirmation;
     - the client confirmation.
4. **Finish.**
   - The client verifies the signature with the pinned key, decapsulates,
     and checks the endpoint confirmation in constant time.
   - It then sends its own confirmation.
   - The endpoint opens no record before that confirmation checks.

### Records

Each record has a 16-byte header and then the ciphertext with its 16-byte tag.
The header holds:
- `DYCR`;
- version `1`;
- a flags byte, with bit 0 marking the end of a message and the other bits
  zero;
- an eight-byte big-endian sequence;
- a two-byte big-endian plaintext length of 1 to 16,384.

- **Encryption.** AES-256-GCM uses the direction's key. The nonce is four
  zero bytes and then the sequence.
- **Associated data.** The whole header, so the end flag is authenticated. A
  message cut short never completes.
- **Order.** Each direction starts at sequence zero. The receiver requires
  the next exact sequence and checks the whole message bound from each header
  before it reads the ciphertext.
- **Limits.** At most 2^20 records per direction. The counter never wraps
  and the key never changes within a connection.
- **Failure.** Any failure closes the connection.

### Messages

A connection carries one request and one response, in the node HTTP adapter's
two request forms and within its bounds.

| Message | Encoding (big-endian lengths) | Bounds |
| --- | --- | --- |
| Request | `1`, method (`1` GET, `2` POST), path (u16), query (u16), body (u32) | path: `/` and visible ASCII, at most 128 bytes; query: at most 16,384; body: at most 1 MiB |
| Response | `2`, status (u16), content type (u16), cache control (u16), body (u32) | status 100–599; header values: printable ASCII, at most 256; body: at most 1.5 MB |

- **GET** is the URI form (`/method?query`) with no body.
- **POST** is JSON-RPC at `/`, with a body and no query.
- **Decoding.** Unknown kinds, trailing bytes and out-of-bound fields are
  refused.

### Properties and limits

- **Endpoint authentication.** Only the holder of the pinned key can sign an
  offer over the client's fresh nonce and key. An interceptor that swaps in
  its own key fails the client's check.
- **Forward secrecy.** Each connection has its own ML-KEM key, dropped after
  decapsulation.
- **Replay.** A replayed hello gets an offer the replayer cannot decapsulate.
  Records are bound to their direction and sequence.
- **Quantum resistance.** ML-KEM-768 and ML-DSA-65 are FIPS 203 and 204
  category 3. The symmetric parts use 256-bit keys.
- **What it does not provide.**
  - Client authentication: transactions carry their own signatures.
  - Padding: record lengths are visible.
  - Resumption: v1 has one exchange per connection.
  - Guaranteed erasure of Rust memory beyond `zeroize`.
- **Endpoint costs.** Each hello costs the endpoint one encapsulation and one
  signature. C-b bounds concurrent handshakes and per-address connections.
  D12-Q01 sets the rates.
- **Review.** The protocol has not had an independent review, which P02
  requires before production.

### Test vector

`fixed_seeds_give_the_recorded_transcript` fixes every input:
- the endpoint key seed `11…`;
- the client nonce `21…`;
- the FIPS 203 `d`, `z` and `m` seeds `22…`, `23…` and `31…`;
- FIPS 204 `rnd` all zero, the deterministic variant.

It records the SHA-256 of the hello, the offer, the finish, one sealed
request and one sealed response, each prefixed with its length:
`74a0f0cae5e6ae2effe560bb09a8191384019f4e99d27d2b9c42c37616c759f8`.

`consensus/cometbft/internal/clientchannel` reproduces the digest
independently. It uses CIRCL's ML-KEM-768 and ML-DSA-65 with Go's HKDF and
AES-GCM, and shares only this specification with the Rust crate.

## Tasks

| Task | Scope |
| --- | --- |
| C-a | This design. The `dytallix-client-channel` crate and the Go cross-check. The contracts toolkit's CosmWasm bridge removed. A stale `ed25519-dalek` entry removed from the module policy. |
| C-b | The endpoint: a channel listener in the node HTTP adapter (seed key file, chain ID, limits), the supervisor's service configuration, the execution policy and interface inventory, and a gateway contract that replaces TLS. |
| C-c | The SDK and CLI: the vendored channel crate; `--endpoint-key`; plain HTTP to loopback only; reqwest TLS and the legacy testnet client removed; a CI check that the SDK lockfile holds no classical crate. |
| C-d | The companion, `dytallix gateway`. |
| C-e | Peer transport wire version 2, with AES-256-GCM records. |

## Classical code left after gap 19

- **The engine fork's source.** The upstream classical packages remain in
  the source: Ed25519, secp256k1, BLS, SecretConnection and libp2p. The
  PQC-only tags make them unbuildable, the Go graph check (G35) enforces
  that, and no production artifact carries them. The default development
  build still compiles them, and removing them from the source is
  undecided.
- **The testnet faucet** (`mainnet/faucet`) is served behind HTTPS and
  funds testnet only.
- **Operator host access (SSH)** uses classical host and user keys. It
  belongs to E05 deployment.

## Code (C-a)

- **`crates/client-channel`** performs no I/O, so any runtime can drive it,
  and it keeps tokio out of the node lockfile:
  - `client_hello`, `ClientStart::finish`, `endpoint_offer` and
    `EndpointPending::finish`;
  - `handshake_payload_len`, which checks a header before the payload is
    read;
  - `Identity::from_seed` and `generate_seed`;
  - `Sealer::seal_message`;
  - `MessageReader`, which says how many bytes to read next and returns the
    message after its authenticated last record;
  - `Request` and `Response` with strict encoding.
- **Providers:**
  - `fips203` 0.4.3, from the author of the `fips204` crate the node and
    SDK already use;
  - `fips204`;
  - `aes-gcm`;
  - `hkdf`;
  - `sha2`.
- **Tests:**
  - a request and a multi-record response cross the channel;
  - only the pinned endpoint completes a handshake: another key, a swapped
    key or another network is refused;
  - a changed ciphertext, signature, confirmation or finish is refused;
  - an encapsulation key above q is refused;
  - handshake headers are checked before reading;
  - records are refused when tampered, reordered, reflected, cut short or
    over the bound;
  - the message codec is strict;
  - the fixed-seed vector is checked in Rust and in Go.
