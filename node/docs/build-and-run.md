# Build And Run

[Docs hub](README.md) | [Repository map](repository-map.md)

## Rust Workspace

```bash
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
cargo fmt --all
```

The workspace builds one configuration: the consensus application
(`dytallix-fast-node` default feature `pqc-consensus`).

Build the consensus application binary used by the bridge:

```bash
cargo build -p dytallix-fast-node --bin consensus_stdio --release --locked
```

### Stopped node check

`dytallix-state-check` runs the application's startup checks on a stopped
node's database, read only, and prints one JSON object with the first
failure in full (E04 gap 15):

```bash
cargo build -p dytallix-fast-node --bin dytallix-state-check --release --locked
dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db HOME/appdb
```

It checks the configuration and genesis bindings, every block record, the
complete supply check and the issuance journal. The root receipt's
authorization and the emergency, upgrade and handover replays need the
owned root helper; the report lists them under `not_checked`.

After a halt, `--restart-target RELEASE_SHA512 --evidence SHA256
[--halted-block-hash HASH] --restart-output DIR` also writes the unsigned
restart authorization for the committed checkpoint and the exact bytes to
sign ([restart runbook](operations/restart.md)).

A stopped application writes no text. Its exit status names the failure
class, which the native supervisor's report repeats
(`application_failure_class`, or `failure_class` for a failed preflight).
The check tool exits with the same status:

| Status | Class | Meaning |
| --- | --- | --- |
| 0 | — | Passed, or no consensus state |
| 1 | — | Unclassified failure |
| 10 | `configuration` | The configuration or genesis is invalid or differs from storage |
| 11 | `history` | A stored block record, its commitments or the application hash differ |
| 12 | `supply` | The supply accounting differs |
| 13 | `replay` | The emergency, upgrade or handover history does not replay |
| 14 | `release` | The running executable is not the committed release |
| 15 | `execution` | Executing a block failed after its inputs passed |
| 16 | `storage` | The database could not be opened, read or written |
| 17 | `resource` | The system refused space, memory, descriptors or tasks |

A failed block method stops the engine, so the application exits with
that method's class. A panic exits with 101.

## Consensus Engine

```bash
cd consensus/cometbft
go vet -mod=readonly ./...
go test -mod=readonly ./...
go build -mod=readonly -o /absolute/bin/ ./cmd/...
go build -mod=readonly -o /absolute/bin/cometbft github.com/cometbft/cometbft/cmd/cometbft
```

See [CometBFT integration](../consensus/cometbft/README.md) to generate a
local four-validator fixture, and
[PQC engine deployment](../deploy/pqc-engine/README.md) for Linux builds and
service templates.

## Checks

```bash
python3 scripts/check_module_boundaries.py
python3 scripts/check_consensus_cargo_profile.py --output /absolute/path/profile.json
python3 -B -m unittest discover -s scripts -p 'test_*.py'
```

`check_consensus_cargo_profile.py` fails if the consensus dependency graph
contains a prohibited classical-cryptography or legacy network crate (gate G35).

CI runs these on every change under `mainnet/`; see
`.github/workflows/mainnet.yml` at the repository root.
