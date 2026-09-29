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
  signature, run on the adapter's single thread. C-b bounds connections in
  all and per client address. D12-Q01 sets the rates.
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
| C-b | The endpoint: a channel listener in the node HTTP adapter (seed key file, chain ID, limits), the key and pin tool, the supervisor's configuration and readiness probe, the interface inventory, and a public endpoint contract that replaces the TLS gateway. |
| C-c1 | The SDK and CLI transport: the vendored channel crate; plain HTTP to loopback only (hyper, no TLS); `--endpoint` takes an endpoint pin file for a remote node; `chain.json` version 2 holds the endpoint key. The default CLI and the Comet and local SDK graphs lose TLS. |
| C-c2 | The legacy testnet client (`network`, `legacy-network`) and reqwest removed, with its TLS; a CI check that no mainnet lockfile holds a classical or TLS crate. |
| C-d | The companion, `dytallix gateway`. |
| C-e | Peer transport wire version 2, with AES-256-GCM records. |

## Endpoint (C-b)

The node's HTTP adapter serves the channel beside its loopback HTTP
listener. Both end in the same engine path: the client socket, the
allowlist, the adapter's body, response and deadline bounds, and its fixed
errors. The rewritten contract is the
[public endpoint contract](rpc-controls-v1.md#public-endpoint-contract).

### Endpoint key and pin

- **The seed.** `HOME/config/client_channel_seed.bin` holds 32 bytes. It is
  a regular file of the service user, mode 0600, with one link.
  - `dytallix-channel-key generate --seed-file FILE` creates it and never
    replaces a file.
  - The adapter refuses a seed equal to the peer transport's
    (`config/pqc_peer_seed.bin`), so the two role keys stay distinct.
- **The pin.** `dytallix-channel-key pin --seed-file FILE --network CHAIN_ID --address HOST:PORT --output FILE`
  writes the file that clients receive:

  ```json
  {"version":1,"network":"CHAIN_ID","address":"HOST:PORT","public_key_base64":"..."}
  ```

  - `address` is an IP literal (IPv6 in brackets) or a DNS name. The key,
    not the address, authenticates the endpoint.
  - The tool reports the key's SHA-256 fingerprint, for people to compare,
    and never prints the seed.
  - Clients trust a pin only as far as the channel through which they
    received it.

### Adapter

`--channel-listen IP:PORT --channel-network CHAIN_ID` enables the listener.
The address is explicit: neither unspecified nor multicast.
- **Bounds.** At most 64 connections in all and 4 per client address.
  `--max-channel-connections` and `--max-channel-connections-per-address`
  can only lower them.
- **Refusals.** A connection over either bound is closed at once.
- **Deadline.** Each exchange ends at the adapter's deadline (10 s).
- **Failures.** A handshake that fails closes the connection with no reply.
- **Request errors.** After the handshake, a refused request gets the
  loopback listener's error in channel form: 400, 413, 501, 502 or 504.
- **Readiness line.** The adapter's readiness line adds the channel address
  and the key fingerprint.

### Supervisor

`adapter_channel` in the service configuration holds `listen` and `pin` (a
pinned input: path, SHA-256, bound), plus optional lowered limits. It
requires `adapter_listen`.
1. **Before start.** The pin's network must be the candidate's chain ID.
   The supervisor then passes the channel flags to the adapter.
2. **Readiness.** After the loopback readiness, it checks that the adapter
   owns the channel listener. It then completes a channel exchange with the
   pinned key. That exchange is a sealed `GET /status`, and it must report
   the chain at the expected height. A seed that does not match the
   published pin, or a broken listener, stops startup at the
   `adapter_channel` stage.
3. **Report.** The service report adds `channel_readiness`.

### Operating an endpoint

1. **Create the seed and the pin.** On the node host, as the service user,
   run `dytallix-channel-key generate`, then `dytallix-channel-key pin` with
   the chain ID and the public address.
2. **Configure the supervisor.** Set `adapter_channel` with the listener
   address and the pin's path and SHA-256.
3. **Publish the pin** through a channel clients already trust, together
   with its fingerprint.
4. **If the key is exposed,** follow
   [key compromise](../operations/key-compromise.md#channel-endpoint-key).
   Version 1 has no key overlap: clients must replace the pin.

### Tests (C-b)

- **Adapter:**
  - a GET and a POST cross the channel to a stand-in engine, which sees the
    client's address;
  - a client that pins another key gets nothing;
  - refused requests get the loopback errors: 501, 400 and 413 at a
    lowered body limit;
  - an unavailable engine is a 502;
  - seed files must be owner-only, single-link and 32 bytes;
  - the channel flags are checked, and a partial set or a seed equal to
    the peer transport's is refused;
  - each client address has its own bound;
  - the key tool generates, pins and never replaces a file.
- **Supervisor:**
  - `adapter_channel` pins a valid endpoint and becomes the adapter flags,
    and bad listeners, limits, bounds and hashes are refused;
  - the probe completes an exchange with the pinned key;
  - another key, another network, a non-200 status or an oversized body
    fails the probe.
- **Channel crate:** the endpoint pin is strict about its fields, version,
  key length, canonical base64, duplicate fields and size.

## SDK and CLI (C-c1)

The SDK vendors `dytallix-client-channel` byte for byte, as it vendors
protocol-types. `scripts/sync_protocol_vendor.py` copies both crates, and
`check_protocol_vendor.py --node-root` checks both in CI.

- **`dytallix_sdk::transport::Endpoint`.**
  - `Loopback` is plain HTTP to a literal loopback address, through hyper's
    HTTP/1 client. Hyper has no TLS code.
  - `Channel` holds an `EndpointPin` and makes one exchange per request:
    the handshake, a sealed JSON-RPC `POST /` and the sealed response.
  - Each request has a 30-second limit and the caller's response bound.
    Non-2xx statuses are errors.
- **`CometClient`.**
  - `new` takes a loopback URL. It refuses HTTPS ("TLS is not supported")
    and remote plain HTTP.
  - `channel` takes a pin; `with_endpoint` takes either kind.
- **The CLI.** `--endpoint` is a loopback URL or the path of an endpoint
  pin file.
  - `config pin-chain` stores a pin file's address and key in `chain.json`,
    version 2. The pin must name the chain being pinned.
  - An `--endpoint` override must also be for the pinned chain.
  - A version 1 `chain.json` still loads. A remote `http://` endpoint in it
    no longer connects; the error says to pin the chain again with the
    endpoint's pin file.
- **Features.** `comet-rpc` (the default CLI), `ordinary-http-only` and
  `strict-local-mldsa65` no longer use reqwest. `cargo tree` shows no TLS
  crate or reqwest in their graphs. `network` and `legacy-network` kept
  reqwest's TLS until C-c2.
- **A repair.** The `dytallix-ordinary-local` binary had not built since
  gap 16's keystore change, and only the standalone SDK repository's CI
  builds it. It builds again and signs from a version 2 keystore.

### Tests (C-c1)

- **SDK transport:**
  - a request crosses the channel to an in-process endpoint, which sees
    `POST /` and the body;
  - a pin with another key or another network gets no answer;
  - statuses and the response bound are checked on both transports;
  - only literal loopback URLs are plain HTTP endpoints.
- **CLI binary** (`tests/channel_cli.rs`):
  - `dytallix ordinary query-account --endpoint PIN_FILE` crosses a real
    channel, and the endpoint sees the ABCI query;
  - a pin with another key is refused;
  - TLS, remote plain HTTP and a missing pin file are refused;
  - `config pin-chain` with a pin file writes version 2 with the key, and
    refuses a pin for another chain.
- **Chain pins:** version 1 and 2, loopback and channel, a noncanonical
  key, and an override for another chain.
- **Existing RPC tests:** the SDK's RPC tests and the CLI's one-step tests
  run unchanged over the new loopback client.

## Legacy testnet client removed (C-c2)

**The SDK** loses:
- the `network` feature;
- the legacy REST client (`client.rs`) and the faucet client
  (`faucet.rs`);
- their fee-estimation methods on the legacy `Transaction`;
- the error variants only they used: `FaucetRateLimited`,
  `FaucetUnavailable`, `NodeUnavailable` and `ContractDeployFailed`.

**The CLI** loses:
- `legacy-network`;
- the commands `init`, `faucet`, `contract`, `node`, `chain`, `dev` and
  `legacy`, with their REST helpers;
- `config set`, `config network` and `config reset`, with
  `~/.dytallix/config.json`, which only those commands read. `config show`
  and `config pin-chain` remain.

**Removed with them:**
- the testnet examples (`first-transaction`, `deploy-contract`,
  `contracts/minimal_contract`);
- the local REST node scripts;
- the public-testnet alignment check (`public-capabilities.json`,
  `check_public_alignment.py`) and its daily smoke workflow.

**Release builds.** The SDK's release workflow had built its binaries with
`legacy-network`. It now builds the consensus-chain CLI.

**Lockfile.** reqwest leaves the workspace, and with it every TLS crate:
the SDK lockfile loses 1,107 lines.
- `sdk/scripts/check_no_classical.py` refuses a lockfile package that is
  one of these:
  - classical signature or key-exchange code: ring, Ed25519, X25519,
    secp256k1, P-256 and the other NIST curves, RSA, DSA, BLS or ECDSA;
  - a TLS or X.509 stack;
  - QUIC.
- Cargo records packages that no feature reaches, so a clean lockfile is
  the strongest source-level statement. Compiled artifacts are T01's.
- The mainnet CI runs the check on all five Rust lockfiles: SDK, node,
  HTTP adapter, contracts and PQC. The SDK's own CI runs it on its own.

**Docs.** The SDK docs no longer describe the public testnet, the faucet,
contracts or a default public endpoint.

## Browser companion (C-d)

`dytallix gateway serve --listen 127.0.0.1:PORT` is the browser path of
decision 2. It listens only on a literal loopback address, which browsers
treat as a secure context, and uses the pinned chain or an `--endpoint`
override.

- **`POST /rpc`** relays one JSON-RPC request unchanged, through
  `CometClient::relay`, over the client channel or loopback HTTP.
  - The body must be `application/json`, at most 1 MiB, and a JSON object
    or array.
  - The node's allowlist and bounds apply.
- **`GET /chain`** reports the pinned network, chain ID, genesis digest and
  endpoint.
- **Other GETs** serve a wallet bundle, but only with
  `--bundle DIR --bundle-sha256 DIGEST`.
  - The digest is the SHA-256 of the bundle's manifest: one
    `<sha256>  <path>` line per file, sorted by path, as `sha256sum`
    prints it. `gateway bundle-digest` computes it.
  - The files are read once, at startup. A digest mismatch stops startup,
    and symlinks are refused.
  - Bounds: 1,024 files and 64 MiB.

**Only the gateway's own pages can use it.**
- **Host.** It refuses a `Host` other than its own literal `IP:PORT`, which
  stops DNS rebinding. `localhost` is refused too.
- **Origin.** It refuses an `Origin` other than its own.
- **Fetch site.** It refuses a `Sec-Fetch-Site` other than `same-origin` or
  `none`.
- **POST body.** It refuses a POST that is not JSON.
- **CORS.** It answers no preflight and sends no CORS headers, so another
  site can neither send it JSON nor read its answers.
- **Response headers.** Every response carries `no-store`, `nosniff`,
  `no-referrer`, `DENY` framing, same-origin resource and opener policies,
  and a Content-Security-Policy. The policy allows the page's own scripts
  and WebAssembly (`'wasm-unsafe-eval'`) and connections to the gateway
  only.
- **Capacity.** It accepts loopback peers only, holds at most 16
  connections, and bounds each at 40 seconds.

It holds no keys: a page signs its own transactions. No Dytallix wallet
bundle exists yet. When one ships, its digest must reach users through a
channel they already trust, as endpoint pins do.

### Tests (C-d)

These run the real binary and speak raw HTTP:
- the gateway's own page and a local program are relayed;
- a foreign `Origin`, `null`, cross-site and same-site fetches, a rebound
  `Host` and `localhost` are refused before the node sees anything;
- a form post (`text/plain`) is refused (415), non-JSON bodies are
  refused (400), and a preflight gets 405 without CORS headers;
- `/chain` reports the pin;
- a pinned bundle is served with its content types and headers, and paths
  outside it are 404;
- a changed file, or a bundle without a digest, stops startup;
- non-loopback listen addresses are refused;
- a page's request crosses the gateway and the client channel to a real
  endpoint.

## Peer transport version 2 (C-e)

Decision 3 is applied to the peer transport (`internal/pqcp2p`). Wire
version 2 differs from version 1 in three ways:
- **Records** are sealed with AES-256-GCM, through Go's `crypto/aes` and
  `crypto/cipher`, in place of ChaCha20-Poly1305. The 12-byte nonce is four
  zero bytes and then the eight-byte sequence. The record header remains the
  associated data.
- **Handshake and record headers** carry version `2`. A version 1 or future
  header is refused before allocation. There is no negotiation or
  downgrade.
- **The suite** is `dytallix-pqcp2p-component-v2/mlkem768/mldsa65/hkdfsha256/aes256gcm`.
  Every signature context and derived key includes it, so a version 1 peer
  never derives version 2 keys. `golang.org/x/crypto/chacha20poly1305`
  leaves the transport.

What did not change: the handshake (ML-KEM-768 with pinned ML-DSA-65 peers),
the key schedule (HKDF-SHA-256), and the record limits.

**Tests.** `TestVersion2RecordsAreAES256GCM` opens a record that `Write`
produced with an independently built AES-256-GCM, using the header as
associated data and the sequence nonce. The header test refuses version 1,
version 3, a bad length and a wrong type. The transport and engine tests
pass with the default tags and with the PQC-only tags.

**Outside the repository.** The E03 negative-peer probe carries a copy of
these files, with a staging-only rejection observer (see the E01
inventory's `peer_key_establishment_and_identity` route). That copy and its
method review need refreshing to version 2 before the next E03 run.

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
