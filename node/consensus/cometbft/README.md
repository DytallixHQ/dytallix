# CometBFT local qualification tools

These tools connect the Rust development application to upstream CometBFT
`v0.40.0`. The upstream commit is
`0880b4d378f347ab16e54ec677ff50d803f37d62`. See `UPSTREAM.json`.
This directory does not authorize production activation or production keys.

The Go bridge uses the official `abci/types.Application` interface and official
ABCI socket server. It does not implement a separate Protobuf decoder or consensus
algorithm. The engine verifies validator votes. The bridge does not create votes,
signatures, or quorum certificates.

## Build

Use the recorded qualification runner to execute builds and tests serially.
Run these commands in this directory after dependency resolution has produced
the pinned `go.sum`:

```sh
go build -mod=readonly -o /absolute/bin/dytallix-comet-bridge ./cmd/dytallix-comet-bridge
go build -mod=readonly -o /absolute/bin/dytallix-comet-fixture ./cmd/dytallix-comet-fixture
go build -mod=readonly -o /absolute/bin/dytallix-comet-verify ./cmd/dytallix-comet-verify
go build -mod=readonly -o /absolute/bin/dytallix-comet-proof ./cmd/dytallix-comet-proof
go build -mod=readonly -o /absolute/bin/cometbft github.com/cometbft/cometbft/cmd/cometbft
go test -mod=readonly ./...
```

The engine module requires Go 1.25.0 or later. A successful build does not establish
distributed execution, signature verification, or crash recovery.

## Generate a disposable fixture

Supply a native development genesis file. The file must be compact JSON without a
trailing newline. Its chain ID must match the explicit command argument. Its
reward validator labels must be `validator-0` through `validator-3`.

```sh
dytallix-comet-fixture \
  --output /private/tmp/dyt-b7-fixture \
  --app-genesis /absolute/native-genesis.json \
  --chain-id dytallix-local-qualification \
  --genesis-time 2026-09-10T00:00:00Z \
  --base-port 28650
```

The output directory must not exist. Use a short absolute path because Unix
socket path lengths are limited. The generator refuses to overwrite keys or
reuse a prior directory. It preserves partial output on failure.

The generator creates:

- `application-config.json`: a shared explicit Rust configuration.
- `native-genesis.json`: an exact copy of the supplied native genesis bytes.
- `node0` through `node3`: independent engine configurations, keys, signing state,
  and private `abci` directories.

Each node has a new ML-DSA-65 validator key with voting power 10. Only
`ml_dsa_65` is allowed for consensus. These keys do not replace or convert account
keys. Each node also has the upstream Ed25519 peer identity. Thus this fixture
does not establish PQC peer transport. The fixture now labels this path
`legacy-cometbft-loopback-only`. It rejects production requests and any
unimplemented P2P profile. It checks loopback isolation before writing configs.

All directories have mode 0700. All generated files have mode 0600. Never copy
these disposable private keys into evidence reports. Keep signing state with its
key when restarting a node. Do not reset signing state.

The fixture uses these explicit development values:

| Parameter | Value |
| --- | --- |
| Validator count and power | 4 validators, power 10 each |
| Initial height | 1 |
| Engine and application block byte limit | 1,048,576 |
| Application transaction byte limit | 262,144 |
| Application transaction count limit | 1,000 |
| Application gas price | 1 |
| Engine evidence byte limit | 65,536 |
| Engine block gas limit | -1, no engine gas limit |
| Commit delay | 1 second |
| Vote extensions | Disabled |
| Mempool | Engine flood mempool |
| State sync | Disabled |

The generator supplies no native issuance budgets, reward parameters, balances,
or vesting schedules. Those inputs come from the supplied native development
genesis. None of the listed values are production defaults.

For node index `i`, the P2P port is `base-port + 10*i`. The RPC port is
`base-port + 10*i + 1`. All listeners use `127.0.0.1`. The four nodes connect only
to their listed loopback peers. Peer exchange is disabled.

## Start a bridge and engine

Start one independent Rust child and bridge for each node:

```sh
dytallix-comet-bridge \
  --socket unix:///private/tmp/dyt-b7-fixture/node0/abci/app.sock \
  -- /absolute/consensus_stdio \
  --config /private/tmp/dyt-b7-fixture/application-config.json \
  --genesis /private/tmp/dyt-b7-fixture/native-genesis.json \
  --db /private/tmp/dyt-b7-fixture/node0/appdb
```

Then start the corresponding engine:

```sh
cometbft start --home /private/tmp/dyt-b7-fixture/node0
```

The bridge accepts only an unused Unix socket path in a directory owned by the
current user with mode 0700. It sets the socket mode to 0600. It rejects TCP
bindings. It sends child diagnostic output to stderr. Child stdout carries only
the protocol below. An interrupted bridge must terminate before its socket is
removed or reused.

The child protocol has an 8 MiB message limit and a 120-second call timeout.
The upstream ABCI socket decoder retains its upstream 2 GiB limit. The child
limit is not an ABCI parser limit. The private local socket and configured engine
block limit constrain this development arrangement.

## Child protocol

Every request and response occupies one JSON line. Calls from the engine's
separate ABCI connections share one mutex. The request envelope is:

```json
{"method":"info","payload":{}}
```

A successful response is `{"ok":true,"result":{...}}`.
A rejected operation is `{"ok":false,"error":"reason"}`.
Malformed responses, trailing JSON, child termination, and timeouts permanently
close the child channel. A valid application rejection does not corrupt the
channel.

Bytes use standard Base64. Hashes use lowercase 64-character hexadecimal strings.
Empty transaction collections use `[]`. All integer fields are JSON integers.

| Method | Request payload | Successful result |
| --- | --- | --- |
| `info` | `{}` | `height`, `app_hash`, `app_version` |
| `init_chain` | `chain_id`, `initial_height`, `app_state_bytes`, `validators`, `evidence_max_age_blocks`, `evidence_max_age_seconds`, `evidence_max_age_nanos` | `app_hash` |
| `check_tx` | `tx`, `type` (`new` or `recheck`) | transaction result |
| `prepare_proposal` | `height`, `time_seconds`, `time_nanos`, `txs`, `max_tx_bytes` | `txs` |
| `process_proposal` | `height`, `time_seconds`, `time_nanos`, `hash`, `txs` | `accept` |
| `finalize_block` | `height`, `time_seconds`, `time_nanos`, `hash`, `txs` | `app_hash`, `tx_results`, optional `validator_updates` |
| `commit` | `{}` | `{}` |
| `query` | `path`, `data`, `height`, `prove` | `code`, `log`, `height`, `value` |

Initialization sends the actual engine evidence duration as whole seconds plus
a nanosecond remainder. The Rust lifecycle profile compares these values with
its frozen release policy. Missing or nonpositive engine limits are rejected.

Each initialization validator has `pubkey_type`, `pubkey_base64`, and `power`.
`pubkey_type` is `ml_dsa_65`. The shared application configuration also has the
explicit `reward_address` mapping. The engine does not supply that mapping.

Each transaction result has `code`, `gas_wanted`, `gas_used`, and `log`.
The optional `data` field uses Base64. This adapter omits it; the bridge returns empty ABCI data.
Gas results must be nonnegative signed 64-bit values.
`FinalizeBlock` must return exactly one result per input transaction.

`FinalizeBlock` returns the prospective application hash. `Commit` persists the
prepared application state. `Info` reports only the committed state. CometBFT
places the application hash from height H in the header of block H+1.

The request `hash` identifies the decided engine block. It is not an application
state hash. The bridge receives decided input from the local engine. The ABCI
last-commit summary does not contain raw validator signatures and cannot serve as
a standalone quorum certificate.

## Verify a real engine commit

```sh
dytallix-comet-verify \
  --rpc http://127.0.0.1:28651 \
  --genesis /private/tmp/dyt-b7-fixture/node0/config/genesis.json \
  --height 3
```

The verifier accepts only a numeric loopback HTTP endpoint. It loads the expected
four ML-DSA-65 keys and powers from genesis. It checks the RPC validator set,
header validator hash, header and commit identity, and the signatures with
upstream `ValidatorSet.VerifyCommit`. A successful JSON result includes the
verified voting power and signature count. A failure exits with a nonzero code.

`header_app_hash_height` states which application height the returned header
hash commits. To verify the application state at H, inspect a verified commit at
H+1. The four equal-weight validators require three signatures for a commit.
Two validators cannot commit a new block.

## Qualification boundaries

The default Batch 7 fixture uses a fixed validator set. The lifecycle profile
accepts validator updates from the Rust application. Consensus parameter updates,
vote extensions, proof queries, and state-sync snapshots remain unsupported.
The Batch 9 local penalty profile accepts duplicate-vote metadata from the engine.
Other profiles reject nonempty misbehavior. The bridge rejects unknown and
light-client misbehavior records. Snapshot offers and chunks are rejected.
The bridge does not issue validator rewards from participation flags.

PQC signer custody, independent operators, production transport, economic
observations, capacity limits, production pause/resume, and production activation
remain separate qualification work. A local four-process test does not qualify
those properties.

## Validator lifecycle fixture

Add `--lifecycle --operators /absolute/operators.json` to the fixture command.
The operators file maps `validator-0` through `validator-4` to funded synthetic
operator accounts. Generate those account keys separately with the Rust fixture
helper. The generator creates six engine nodes. Only nodes 0 through 3 enter
genesis. Node 4 supplies a registration key. Node 5 supplies a rotation key.
`lifecycle-public-keys.json` contains the six public keys.

Initial voting powers equal funded native reward-position principal in uDGT.
The lifecycle fixture uses a minimum self-bond of 10 uDGT, active-set capacity 8,
evidence limits of 3 blocks and 3 seconds, and processing margins of 1 block and
1 second. These are synthetic test values, not production defaults.

The bridge bounds initialization to 64 positive ML-DSA-65 validators and rejects
duplicate addresses or a total above the engine limit. The Rust configuration
and runtime apply their own lower configured limits. FinalizeBlock can return
up to 128 distinct `validator_updates`. Each entry contains `pubkey_type`,
`pubkey_base64`, and signed integer `power`. Zero removes a key. Positive power
adds or updates a key. The application must validate the final scheduled set.
The engine activates updates returned at H at H+2.

Generate the domain-bound proof payload with the Rust helper, then sign it:

```sh
dytallix-comet-proof \
  --key /private/tmp/dyt-b8-fixture/node4/config/priv_validator_key.json \
  --payload /private/tmp/register-proof.bin
```

The signer requires a local lifecycle fixture configuration and a regular 0600
key file. It verifies the proof domain, chain, operator, validator ID, public key,
operation, nonce, expiry, and amount structure. It writes one Base64 signature
to stdout. It does not modify signing state or print private key data.

Add `--dynamic` to the verifier command for changing validator sets. This mode
checks every height from 1 through the requested height, up to 256. Genesis
anchors the first set. Each preceding signed header authenticates the next
validator set. The verifier checks full commits, block linkage, increasing time,
and prior application-hash and transaction-result commitments. RPC validator
responses are never independent trust anchors. This verifies signed commitments;
it does not re-execute application transactions or qualify production operation.

## Batch 9 local penalty fixture

Add `--penalty` to `--lifecycle --operators <path>` to create a disposable penalty
fixture. The application profile is `cometbft-penalty-local-qualification`.
Its embedded lifecycle profile remains `cometbft-lifecycle-local-qualification`.
The explicit fixture penalty is 1/20. Production activation is false.
The application records penalty principal in a reserve with no sweep authority.
These fixture values do not approve production penalty rates or withdrawals.

PrepareProposal, ProcessProposal, and FinalizeBlock forward the same optional
`misbehavior` array. Each item has `kind` (`duplicate_vote`), `validator_address`
(40 lowercase hexadecimal characters), `height`, `time_seconds`, `time_nanos`,
`power`, and `total_power`. Empty arrays are omitted to preserve earlier request
serialization. The bridge limits each request to 64 records. Rust checks the
historical address, power, timestamp, liability, and application profile.

These records are metadata from the controlled engine/application boundary.
They contain no raw evidence hash, round, votes, or signatures. Application
settlement tests with synthetic records do not verify engine evidence signatures.
A fault-free network run does not qualify cryptographic fault processing.

Light-client evidence needs separate qualification. Its reported height can be
an earlier common height. Some engine light-client cases identify no accused
validators and therefore produce no ABCI misbehavior records. This bridge cannot
observe those cases through the ABCI metadata alone. Do not interpret an empty
array as proof that the block contains no engine evidence.

Keep state sync disabled. Withdrawal qualification depends on historical
validator identities, stake exposure, block times, and incident records across
the full evidence window. A snapshot of only current balances is insufficient.


## Approved PQC profile: local component status

Launch status remains **NO GO**. The approved target uses ML-KEM-768 for P2P
key establishment and ML-DSA-65 for operational signatures. SLH-DSA remains a
separate root and exceptional authorization mechanism. This does not prohibit
symmetric encryption or hashing merely because those primitives predate PQC.

`internal/pqcp2p` implements an isolated session-establishment component. It uses
Go `crypto/mlkem` and CIRCL `sign/mldsa/mldsa65`. It requires full peer public-key
pins from authenticated configuration. It binds the network, protocol suite,
roles, fresh nonce, ephemeral KEM key and ciphertext into authenticated messages.
It derives directional keys with HKDF-SHA256 and checks mutual HMAC-SHA256 key
confirmation. Each attempt is single use. `Close` discards abandoned state and
clears pending derived keys. Go does not guarantee erasure of private-key objects.

The component has no network codec or record layer. The upstream engine does not
call it. It does not replace SecretConnection and does not establish FIPS 140
module validation or production protocol security. Component tests cannot open
the production gate.

The fixture exposes `--p2p-profile legacy-cometbft-loopback-only`. All other values
fail. `--production` always fails before fixture creation. The generator disables
libp2p and remote signer listeners. It rejects public or DNS-based peers, public
listeners, discovery, and TCP ABCI in generated fixtures. These checks apply to
the generator. They do not prevent a user from editing configs or running the
upstream engine separately.

Before integration, approve a versioned wire protocol, record framing, deadlines,
resource limits, full-key peer discovery and rotation, key custody, replay rules,
restart rules, and interoperability evidence. Replace or exclude upstream TCP
remote signer, Noise and QUIC/TLS paths. Do not wrap SecretConnection in PQC and
claim that the inner classical trust dependency has been removed.

Run the component and fixture tests locally:

```sh
go test -mod=readonly -count=1 -v ./internal/pqcp2p ./cmd/dytallix-comet-fixture
go test -mod=readonly -race -count=1 ./internal/pqcp2p
```

The launch workspace records the source inventory, limits, exact tests and file
hashes under `decision-register/pqc-profile/p2p/`.
