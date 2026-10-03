# E05: host configuration from the pin plan

Engineering task E05 (genesis from approved inputs), host files. The
generator turns the published pin plan, the approved per-host values and the
engine genesis into each host's engine files: `config.toml`,
`pqc_transport.json` and the host's production binding. It checks each host
against the engine's own start checks for the production transport profile,
and writes nothing unless every host passes. The same inputs always give the
same bytes.

Generating accepts nothing. Approved values for every open input, the
accepted pin plan, the reviewed host files and gate acceptance remain
required.

## Pipeline

```text
launch/E05_VALUES.json (approved values) ┐
launch/hosts/PROPOSALS.json (labeled)    ┘─ resolve_host_values.py ─> HOST_VALUES.json + resolution report
PIN_PLAN.json (D12-Q01, public)          ┐
HOST_VALUES.json                         ├─ dytallix-host-config ─> hosts/LABEL/{config.toml, pqc_transport.json, binding.json}
genesis.json (the built engine genesis)  ┘                          PIN_PLAN_BINDINGS.json
```

1. **Resolve** (`tools/mainnet-preparation/resolve_host_values.py`). The 34
   per-host engine settings (the `config.toml` and `pqc_transport.json`
   values in `E05_VALUES.json`) each come from exactly one source: an APPROVED
   entry, or a labeled entry (`PROPOSED` or `MEASURE_PLACEHOLDER`) in
   `launch/hosts/PROPOSALS.json` for a value still open. The rules match the
   genesis resolver. The report is `production_eligible` only when every value
   is approved. Today 30 are approved and 4 are measurement placeholders
   (mempool size and bytes, P2P send and receive rates; T02, T05).
2. **Generate** (`dytallix-host-config`, in the engine module). It refuses a
   plan that breaks the approved mesh rules, renders each host's
   configuration with the engine's own template and reads it back with the
   engine's own decoder, then runs the engine's start checks on the
   configuration, the transport file and the host's public keys.

```text
python3 -B tools/mainnet-preparation/resolve_host_values.py \
  --values ../launch/E05_VALUES.json --proposals ../launch/hosts/PROPOSALS.json \
  --host-values HOST_VALUES.json --resolution HOST_VALUES_RESOLUTION.json
cd consensus/cometbft
go build -mod=readonly -tags production -o /absolute/bin/ ./cmd/dytallix-host-config
dytallix-host-config --plan PIN_PLAN.json --values HOST_VALUES.json \
  --genesis genesis.json --out HOSTS_DIR
```

Build the generator with `-tags production` for a production chain: like the
engine, a development build refuses a chain ID naming mainnet or production.
`HOSTS_DIR` must not exist. Every file is written owner-only.

## The pin plan

Schema `dytallix.pin-plan.v1`, strict JSON with every field present. It holds
public keys and addresses only.

```json
{
  "schema": "dytallix.pin-plan.v1",
  "chain_id": "the engine genesis chain ID",
  "hosts": [{
    "label": "sentry-1a",
    "operator": "operator-1",
    "role": "validator | sentry | endpoint",
    "home": "/absolute/node/home",
    "p2p": "IP:port",
    "channel": "IP:port on an endpoint, else null",
    "peer_public_key_base64": "from dytallix-peer-seed generate",
    "validator_public_key_base64": "from dytallix-validator-key generate",
    "pins": ["labels of the hosts it peers with"],
    "state_sync": null
  }]
}
```

The generator refuses:

- **Addresses.** A P2P or channel address that is not a canonical global
  unicast IP and a port from 1024, two hosts on one IP (one IP per node, P01,
  30 September 2026), a channel on a sentry or validator or missing on an
  endpoint, a channel off the node's IP or on its P2P port, and pins across
  address families (the host firewall admits one family).
- **Keys.** A key that is not canonical base64 of an ML-DSA-65 public key,
  and any key used twice in the plan, as a peer key or a validator key.
- **Roles.** A validator whose key is not in the genesis validator set, a
  sentry or endpoint whose key is, and a genesis validator with no
  validator host.
- **Pins** (the published partial mesh, P01, 30 September 2026). One to 64
  pins per host; an unknown, repeated or self pin; a pin the other host does
  not return; a validator pinning anything but its own operator's sentries;
  an endpoint pinning anything but sentries.
- **State sync.** A trust height below 1 or a trust hash that is not 32 bytes
  of hex. The configuration's trust period must stay below the genesis
  evidence age.

## What each host gets

- **`config.toml`.** The engine's template with every approved value, the
  role's `double_sign_check_height` (10 on validators, 0 on sentries and
  endpoints), the host's label as moniker, local Unix ABCI under the home,
  goleveldb, JSON logs, and:
  - P2P listening on the host's address, peer exchange off, no seeds or
    external address, strict address book, no duplicate IP, and the pins as
    `persistent_peers` (`id@address`) with the approved 60 s redial cap.
  - RPC on a loopback address it never serves (the engine serves owner-only
    Unix sockets), no gRPC, pprof, unsafe RPC, browser origin or Prometheus.
  - The flood mempool with recheck.
  - State sync off, or on with the plan's trusted block and no RPC servers:
    the host takes its operator's light blocks (state sync v1).
- **`pqc_transport.json`.** Canonical compact JSON in the production profile
  `dytallix-pqc-production-v1`: the host's peer key, each pin's key, peer ID
  and address, and the approved 5,000 ms handshake timeout.
- **`binding.json`.** The host's production binding: the role, the chain and
  the SHA-256 of the configuration, genesis and transport files and of the
  peer and validator public keys. It is what `dytallix-peer-seed binding`
  prints on the host once the files are installed.
- **`PIN_PLAN_BINDINGS.json`.** Every host's binding, its digest, its pins
  and its `firewall` block (`transport_sha256`, `p2p_listen`,
  `public_listeners`), the input the host policy renderer's routed mode
  takes, with the digests of the plan, the values and the genesis.

The supervisor configuration is not generated here: it pins private files
(the peer seed and the validator key), so the operator writes it on the host.

## On each host

1. **Keys.** `dytallix-peer-seed generate --home HOME` and
   `dytallix-validator-key generate --key-file FILE --state-file FILE` create
   the keys on the host. Only their public keys enter the plan.
2. **Files.** Once the plan is accepted and generated, install the host's
   `config.toml`, `pqc_transport.json` and `binding.json` and the engine
   genesis under `HOME/config`, owner-only.
3. **Binding.** `dytallix-peer-seed binding --home HOME --role ROLE` must
   print the published binding byte for byte. The engine (`--binding`) and
   the supervisor check it again at every start.
4. **Policy.** Render the unit, the AppArmor profiles and `host-firewall.nft`
   with `tools/native-execution-policy` in routed mode, passing the host's
   exact transport file and its `firewall` block.

A pin change is a new plan: regenerate, review, reinstall, re-render the
firewall and restart.

## Publication

The production pin plan names hosts and addresses, so it is released with
the network configuration, outside this repository. Only its digest, the
bindings' digest and the generator's commit enter the repository.

## Rehearsal

`tools/mainnet-preparation/fixtures/host-config-rehearsal/` holds a
synthetic plan for the production-profile genesis rehearsal on the staging
chain `dytallix-staging-1`: four operators, four validators with the
rehearsal's genesis keys, five sentries and one endpoint, on documentation
addresses (RFC 5737). Its peer keys and non-validator keys are SHAKE-256
bytes, so no private key exists for any of them. It also holds the resolved
host values and the generated `PIN_PLAN_BINDINGS.json`.

- `go test ./cmd/dytallix-host-config` regenerates the plan and the bindings
  and compares them with the committed files
  (`DYTALLIX_UPDATE_HOST_FIXTURE=1` rewrites them), and checks the plan rules,
  the strict inputs and the output.
- `go test -tags production ./cmd/dytallix-host-config` installs generated
  files on five hosts with seed-derived test keys and loads each with the
  production engine. Each starts, and the binding the engine computes is the
  published one.
- `test_resolve_host_values.py` checks the resolver and the committed
  values.
