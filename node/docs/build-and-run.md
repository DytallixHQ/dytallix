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
