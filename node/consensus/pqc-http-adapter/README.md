# PQC HTTP adapter

This standalone Rust workspace provides a loopback HTTP/1 adapter for the engine's Unix RPC profile. Hyper 1.9.0 parses HTTP. The Go engine interprets JSON-RPC and owns chain state. The adapter authorizes nothing; a chain's production start rests on its signed root genesis.

**One profile per build** (production activation v1). A development build serves only `dytallix-pqc-http-local-v1` and reports `EXPERIMENTAL_LOCAL_ONLY` on its ready line. A production build (`--features production`) serves only `dytallix-pqc-http-production-v1` and reports `READY`. The native supervisor passes its own build's profile. Both builds reject a `--production` flag: the build selects production, never a flag.

The adapter contains no TLS implementation dependency. Its selected executable and operating-system providers still require inventory review. A plain loopback listener does not provide secure hosted-wallet ingress; remote clients use the optional client channel listener below. No independent acceptance is supplied here.

## Start order

1. Start the Go engine with its `dytallix-pqc-unix-v1` RPC profile.
2. Verify that `HOME` and `HOME/data` have mode 0700 and belong to the service user. The engine must create `HOME/data/rpc.sock` with mode 0600. The adapter uses only this client socket, which serves the client method allowlist; the engine's `rpc-operator.sock` is for the node's owner and is never forwarded (`docs/architecture/rpc-controls-v1.md`).
3. Start this adapter with the same user and canonical home path:

```text
dytallix-pqc-http-adapter --profile dytallix-pqc-http-local-v1 --home /absolute/private/home --listen 127.0.0.1:26657
```

Use an explicit numeric loopback address, and the profile of the build. Port zero is permitted for disposable tests. One JSON line on standard output reports the bound address and the build's status. Readiness means the adapter bound its socket and checked the local IPC path. It does not prove that consensus is healthy. The adapter neither creates nor removes the engine socket.

The adapter has no durable state. Stopping it closes its connections. The Go engine retains transaction and consensus state. Restart and commitment qualification must use the actual engine, not only the synthetic fixture below.

## Interface and limits

HTTP supports JSON-RPC POST at `/` and URI GET at `/method_name`. The adapter preserves JSON body bytes with canonical standard Base64 in IPC. It forwards the raw query string. It derives the remote address from the accepted loopback connection and ignores forwarded-address headers.

The version-1 IPC message uses four-byte big-endian length framing. Requests contain exactly `version`, `method`, `path`, `query`, `body_base64`, and `remote_addr`. Responses contain exactly `version`, `status`, `headers`, and `body_base64`. The write side stays open while awaiting the response. Dropping the IPC stream tells the engine that the adapter disconnected.

Each IPC frame is at most 2,097,152 bytes. Decoded request and response bodies are at most 1,048,576 and 1,500,000 bytes. The RPC path is at most 128 ASCII bytes. The raw query is at most 16,384 bytes. The response header allowlist contains only `Content-Type` and `Cache-Control`. Unknown or duplicate response fields and noncanonical Base64 fail closed. The adapter owns CORS headers.

The adapter permits 32 accepted connections at once. It closes excess connections. Each accepted connection has a ten-second total deadline and one HTTP request. Keep-alive is disabled. Hyper limits headers to 64 and its HTTP/1 buffer to 65,536 bytes. These are prototype resource bounds, not approved production capacity. Per-request IPC path checks require real private directories and a private socket owned by the same user. They assume the service account and private directory owner are trusted. They do not defend against that owner replacing its own files.

CORS permits only `http://127.0.0.1:4173`. Native requests without Origin remain supported. OPTIONS accepts GET or POST and the `content-type` request header. Credentials and arbitrary origins are not enabled. CORS does not authenticate callers.

The adapter returns 501 for WebSocket upgrades, `/websocket`, root browsing and URI-form POST. It does not implement HTTP/2, TLS, compression, signing, custody, or application RPC methods. Unsupported features remain available only through separately selected historical profiles. This does not approve their removal from a required production release.

## Client channel listener

Remote clients reach the node through the post-quantum client channel, not TLS (E04 gap 19; [client channel v1](../../docs/architecture/client-channel-v1.md)). Add these flags:

```text
--channel-listen IP:PORT --channel-network CHAIN_ID
```

- **Address.** An explicit IP: not unspecified, not multicast.
- **Engine path.** The listener sends each request down the same path as HTTP: the client socket, the allowlist, the adapter's bounds and its fixed errors.
- **Connection bounds.** At most 64 connections in all and 4 per client address. `--max-channel-connections` and `--max-channel-connections-per-address` only lower these.
- **The key.** The endpoint signs with the seed in `HOME/config/client_channel_seed.bin`: 32 bytes, mode 0600, one link, owned by the service user. The adapter refuses a seed equal to the peer transport's.
- **Readiness line.** It adds `channel_listen` and `channel_key_sha256`.

`dytallix-channel-key` is built from this package. Run it as the service user:

```text
dytallix-channel-key generate --seed-file HOME/config/client_channel_seed.bin
dytallix-channel-key pin --seed-file HOME/config/client_channel_seed.bin --network CHAIN_ID --address HOST:PORT --output pin.json
```

`generate` never replaces a file, and neither command prints the seed. The supervisor pins `pin.json` (`adapter_channel`) and probes the listener with its key before readiness.

## Status page

An endpoint can serve a read-only status page for a free uptime checker (P01, 3 October 2026). Add `--status-listen IP:PORT`:

- **Address.** An explicit address and port of its own, not the loopback listener's or the channel's. A production build refuses loopback and link-local addresses; the supervisor requires the node's P2P IP.
- **What it serves.** `GET /status` only, answering `{"chain_id","height","time"}` from the engine's local `/status` route, with `Cache-Control: no-store`. Every other path or method gets a fixed 404 or 405, and an unavailable engine a 503.
- **What it is not.** It uses no cryptography and takes no input, so it adds nothing to the PQC-only boundary (G35). It is unauthenticated: a liveness hint for monitoring, never a source of chain state for clients.
- **Bounds.** At most 8 connections, one request each, a 2 s deadline, 16 headers and an 8 KiB header buffer.
- **Readiness line.** It adds `status_listen`.

## Build and local tests

The package has an independent `[workspace]` and lockfile. It does not join or modify the node workspace. Dependencies use the existing pinned versions. Use one Cargo job and keep at least 2 GiB free disk.

```text
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo build --locked --offline --release
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --locked --offline --release
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --locked --offline --release --features production
python3 -B tools/test_loopback.py --binary target/release/dytallix-pqc-http-adapter --output /new/absolute/loopback-result.json
```

`tools/test_loopback.py` starts the adapter without the owner guard's descriptors. The adapter now admits itself through that guard (Linux only), so the script no longer runs as is. The Rust tests cover all three listeners in both builds.

The loopback tests start the actual adapter and a private synthetic IPC fixture. They verify POST, chunked bodies, GET queries, CORS, unsupported interfaces, response-header policy, restart, and path permissions. They do not simulate consensus or prove transaction commitment. The fixture creates no private signing keys and removes its temporary directory.

Production work remains: Linux artifact and provider review, actual-engine compatibility, process supervision, secure hosted ingress, reviewed endpoint policy, load qualification, independent review, and final acceptance through the canonical launch process.
