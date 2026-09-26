# Experimental engine transport

The `dytallix-pqc-engine` command installs the experimental PQC transport in the actual Comet TCP peer path. It uses `pqcp2p.Upgrade` for inbound and outbound connections before the Comet `NodeInfo` exchange. A rejected PQC connection cannot retry through the legacy transport.

The selected profile is `dytallix-pqc-loopback-v1`. The command requires an existing private fixture home. It does not generate keys, reset signer state, or provide a production mode.

## Interfaces

The copied upstream engine adds three interfaces:

- `p2p.AuthenticatedConn`: `net.Conn` plus `RemotePubKey() crypto.PubKey`.
- `p2p.AuthenticatedConnUpgrade`: a callback with the raw connection, optional outbound address, and timeout.
- `node.WithAuthenticatedTransport`: installs the callback before engine listeners start.

The fork keeps the engine's existing authenticated-key checks against the dialed peer ID and the peer's `NodeInfo`. `peerConn.ID` now reads the selected authenticated connection interface. The default upstream command retains its historical transport. The new command always installs the PQC callback.

`internal/enginepqc` checks the complete peer configuration before it constructs the engine. Each peer entry contains its full 1,952-byte ML-DSA-65 public key, derived peer ID and exact local address. Outbound connections select the complete key through the configured ID and address. Inbound authentication accepts only the complete configured keys. Peer IDs alone do not authorize connections.

The adapter limits concurrent handshakes to eight. It acquires a slot before cryptographic work and releases the slot after every callback result. It rejects excess handshakes. It does not wait in an unbounded queue.

## Commands

Build from this directory:

```sh
go build -mod=readonly -o /private/tmp/dytallix-pqc-engine ./cmd/dytallix-pqc-engine
go build -mod=readonly -o /private/tmp/dytallix-comet-fixture ./cmd/dytallix-comet-fixture
```

Add `--p2p-profile dytallix-pqc-loopback-v1` to the existing fixture generator arguments. The new mode creates independent ML-DSA-65 peer identities and `config/pqc_transport.json`. Validator identities remain separate ML-DSA-65 keys. The historical fixture mode remains the default for old diagnostic workflows.

Start each generated engine after its ABCI bridge is ready:

```sh
/private/tmp/dytallix-pqc-engine start --home /ABSOLUTE/PRIVATE/HOME --p2p-profile dytallix-pqc-loopback-v1
```

`config/pqc_transport.json` uses compact JSON with this field order:

```text
version, profile, network, local_public_key_base64,
peers [{id, public_key_base64, address}], handshake_timeout_ms
```

The network is the exact genesis chain ID. Each configured peer must also appear once in `persistent_peers` with the same address. Restart retains the existing key, signer state and databases.

The command rejects remote signing, classical peer identities, non-ML-DSA-65 validator keys, libp2p, discovery, state sync, unsafe RPC, TLS listeners and nonlocal P2P/RPC addresses. Its only optional browser origin is `http://127.0.0.1:4173`.

The public `experimental_pqc_engine_ready` event records the selected profile, suite, node ID, allowed peer IDs, full-key size and handshake limit. `PQC transport authenticated` records successful transport authentication. Actual connected-peer evidence comes from the engine's `net_info` response.

## Boundaries

`upstream/` is a copy of the local `cometbft@v0.40.0` module cache. `UPSTREAM_COPY_MANIFEST.json` records each file before integration edits. It does not establish that the cache equals the remote release tag. The copy retains its license and provenance files.

The engine still contains legacy cryptographic code and dependencies. This integration does not pass the no-classical-artifact gate. The protocol remains experimental and requires independent review. The local wrapper's input guards do not establish complete packed-private-key consistency or replace trusted fixture-key custody. No production qualification follows from a local transport or engine test.
