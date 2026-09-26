# Experimental PQC HTTP adapter

This standalone Rust workspace provides a loopback HTTP/1 adapter for the experimental Go Unix RPC profile. Hyper 1.9.0 parses HTTP. The Go engine interprets JSON-RPC and owns chain state. This prototype cannot authorize production. The command rejects `--production` and requires an explicit experimental profile.

The adapter contains no TLS implementation dependency. Its selected executable and operating-system providers still require inventory review. A plain loopback listener does not provide secure hosted-wallet ingress. No independent acceptance is supplied here.

## Start order

1. Start the Go engine with its `dytallix-pqc-unix-v1` RPC profile.
2. Verify that `HOME` and `HOME/data` have mode 0700 and belong to the service user. The engine must create `HOME/data/rpc.sock` with mode 0600.
3. Start this adapter with the same user and canonical home path:

```text
dytallix-pqc-http-adapter --profile dytallix-pqc-http-local-v1 --home /absolute/private/home --listen 127.0.0.1:26657
```

Use an explicit numeric loopback address. Port zero is permitted for disposable tests. One JSON line on standard output reports the bound address and experimental status. Readiness means the adapter bound its socket and checked the local IPC path. It does not prove that consensus is healthy. The adapter neither creates nor removes the engine socket.

The adapter has no durable state. Stopping it closes its connections. The Go engine retains transaction and consensus state. Restart and commitment qualification must use the actual engine, not only the synthetic fixture below.

## Interface and limits

HTTP supports JSON-RPC POST at `/` and URI GET at `/method_name`. The adapter preserves JSON body bytes with canonical standard Base64 in IPC. It forwards the raw query string. It derives the remote address from the accepted loopback connection and ignores forwarded-address headers.

The version-1 IPC message uses four-byte big-endian length framing. Requests contain exactly `version`, `method`, `path`, `query`, `body_base64`, and `remote_addr`. Responses contain exactly `version`, `status`, `headers`, and `body_base64`. The write side stays open while awaiting the response. Dropping the IPC stream tells the engine that the adapter disconnected.

Each IPC frame is at most 2,097,152 bytes. Decoded request and response bodies are at most 1,048,576 and 1,500,000 bytes. The RPC path is at most 128 ASCII bytes. The raw query is at most 16,384 bytes. The response header allowlist contains only `Content-Type` and `Cache-Control`. Unknown or duplicate response fields and noncanonical Base64 fail closed. The adapter owns CORS headers.

The adapter permits 32 accepted connections at once. It closes excess connections. Each accepted connection has a ten-second total deadline and one HTTP request. Keep-alive is disabled. Hyper limits headers to 64 and its HTTP/1 buffer to 65,536 bytes. These are prototype resource bounds, not approved production capacity. Per-request IPC path checks require real private directories and a private socket owned by the same user. They assume the service account and private directory owner are trusted. They do not defend against that owner replacing its own files.

CORS permits only `http://127.0.0.1:4173`. Native requests without Origin remain supported. OPTIONS accepts GET or POST and the `content-type` request header. Credentials and arbitrary origins are not enabled. CORS does not authenticate callers.

The adapter returns 501 for WebSocket upgrades, `/websocket`, root browsing and URI-form POST. It does not implement HTTP/2, TLS, compression, signing, custody, or application RPC methods. Unsupported features remain available only through separately selected historical profiles. This does not approve their removal from a required production release.

## Build and local tests

The package has an independent `[workspace]` and lockfile. It does not join or modify the node workspace. Dependencies use the existing pinned versions. Use one Cargo job and keep at least 2 GiB free disk.

```text
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo build --locked --offline --release
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --locked --offline --release
python3 -B tools/test_loopback.py --binary target/release/dytallix-pqc-http-adapter --output /new/absolute/loopback-result.json
```

The loopback tests start the actual adapter and a private synthetic IPC fixture. They verify POST, chunked bodies, GET queries, CORS, unsupported interfaces, response-header policy, restart, and path permissions. They do not simulate consensus or prove transaction commitment. The fixture creates no private signing keys and removes its temporary directory.

Production work remains: Linux artifact and provider review, actual-engine compatibility, process supervision, secure hosted ingress, reviewed endpoint policy, load qualification, independent review, and final acceptance through the canonical launch process.
