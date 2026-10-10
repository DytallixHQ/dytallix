# Public runtime binding review

`check_bindings.py` checks supplied data. It never creates genesis, starts a node, signs a record, or enables production.

Run from the node repository:

```text
python3 -B tools/mainnet-preparation/check_bindings.py \
  --bindings BINDINGS.json --records PRODUCTION_INPUTS.json \
  --native NATIVE_GENESIS.json --application APPLICATION_CONFIG.json \
  --service SERVICE_CONFIG.json --engine ENGINE_GENESIS.json --manifest BUILD_MANIFEST.json
```

The native, application, service, engine genesis (`--engine`) and builder manifest (`--manifest`) files are optional. Without them, the result lists the missing runtime bytes. Exit code 1 means the review remains BLOCKED. Exit code 2 means a supplied field or source binding failed validation. No exit code grants acceptance.

The records argument uses the existing `PRODUCTION_INPUTS` record IDs and row arrays. Run the separate intake checker to validate the complete record schema and acceptance state. This tool checks only the records used by its supported bindings. A record reference does not establish approval.

## Supported typed inputs

| Input | Check |
|---|---|
| Exact source bytes | Match SHA-256 for the supplied records, native genesis, and application configuration. Match the application's embedded native-genesis digest. |
| Native amounts | Require decimal strings, u128 bounds, the existing DGT cap, unique accounts, explicit vesting, and checked stake funding. Match funded delegations to reward positions. Report `full_dgt_issuance` as missing unless genesis issues the whole 1,000,000,000 DGT: all DGT is issued at genesis and nothing mints it later (D05-Q02). |
| Native reward and issuance inputs | Check the current development versions, activation height, decimals, resource limits, validator population, controller bounds, and epoch budget. |
| Application configuration | Check the profile, chain ID, gas and byte limits, canonical ML-DSA-65 public key encoding, unique identities, positive bounded power, and reward-validator agreement. Report `recovery_and_ordinary_profiles` as missing unless both profiles are present: they are the only user-transaction paths (E04 gap 14). Report `lifecycle_and_penalty_profiles` as missing unless both are present: they penalize double-signing and allow withdrawals (penalties v1). |
| Full configuration (E05-d2) | `config_checks.review` re-derives, independently of the node: lifecycle, penalty, recovery, ordinary, governance and root-control structure; every account address from its origin key (SHA3-256 and Bech32m); the validator-proof profile digest; the E05-a rules (fee caps, transport bound, governance thresholds and bounds, root control bounds, evidence seconds); validator power as bonded stake and self-bonds at the minimum; recovery accounts equal to native accounts; root policies bound to the native genesis; upgrade keys disjoint from emergency keys; and the development gates the node still requires. |
| Engine genesis (E05-d2) | The engine genesis ends with the exact native genesis as `app_state`; chain, initial height, key profile, evidence limits equal to the lifecycle's, validator addresses and powers equal to the application's. Its digest binds through `source_digests.engine_genesis_sha256`. |
| Builder manifest (E05-d2) | Each file's size, SHA-256 and SHA-512, and the build digest. |
| Service configuration | Report `service_configuration` as missing unless supplied, and `metrics_output` unless it sets `metrics` (an absolute `directory` and `interval_seconds` from 1 to 3600): the incident runbooks read the metrics files (E04 gap 15). No other service field is reviewed. |
| Chain identity | Resolve the D13-Q02 identity policy reference to a typed public document. Match its chain ID to both runtime files. |
| Genesis time and governance | `genesis_time` equals the engine genesis's; `governance_parameters` equals the configuration's governance section exactly. |
| Beneficiary bindings | Resolve D08-Q01 account references. Match amounts, explicit vesting documents, staking permission, and operator-specific funded delegations. Require every native account and supplied allocation row to map exactly once. |
| DRT bootstrap | Match each D08-Q03 row to its recipient account and policy reference. Reconcile all rows and the policy total with native DRT balances. |
| Validator bindings | Resolve D09-Q02 operator references to exact public key documents. Match keys to the application validators. Reject duplicate operator or validator mappings. |
| Public transport manifest | Check the Go loopback profile, timeout and peer limits, full public key pins, SHA-256-derived 20-byte peer IDs, addresses, and persistent-peer agreement. |

`source_digests` identifies exact input bytes. `runtime_inputs` retains the eleven existing preparation field names. `public_documents` resolves public references with typed payloads and hashes. The fixture shows every supported field shape.

Public document entries contain `reference`, `sha256`, and `document`. Supported document kinds are `account`, `validator_key`, `peer_key`, `vesting_terms`, and `identity_policy`. Each kind rejects unknown fields. Private key fields are not accepted. Hash each document as sorted-key compact ASCII JSON with one final newline. This package convention is not a general JSON canonicalization standard.

No reference causes a file or network fetch. The tool reads only explicit command arguments. It limits each input to 8 MiB and JSON nesting to 64 levels. Duplicate keys and non-finite numbers fail.

## Consumer limits

These checks implement a strict supported subset of the current Rust and Go consumers. The native subset requires explicit `udgt` and `udrt` fields, vesting, delegations, reward state, and issuance inputs. It rejects omitted development defaults and alternate delegation representations. Canonical decimal strings are stricter than the native parser's digit-only strings.

This adapter permits one validator per operator record. It does not establish the production policy for operators that control several validators. The record format permits one initial operator delegation per beneficiary row. This version does not combine beneficiary rows or invent a multi-operator record. Allocation amounts must match native genesis credits exactly. D05-Q02 (P01, 29 September 2026) excludes a partial genesis mint, so the review requires the full total rather than inferring it from the one-billion-token allocation.

The application check enforces the consumer's 8 MiB limit (`MAX_CONFIG_BYTES`, E05-a) on the exact supplied file bytes; the configuration carries the genesis recovery accounts, about 15 KB each. The public transport check covers the typed manifest and peer tuples. Scoped IPv6 addresses are rejected because the Go loopback parser does not accept them. It does not inspect a full Go TOML configuration, its canonical transport file bytes, loaded private keys, private file permissions, or the complete engine genesis. It does not test a handshake or key possession.

## Unsupported fields remain open

SLH-DSA root authorization and approval-bundle semantics have no typed adapter in this tool. They remain UNSUPPORTED when populated. No populated string or object can make `runtime_complete` true.

The tool does not establish production activation, custody, signature authenticity, validator admission, stake-to-power policy, or independent review. The supported runtime profiles remain local development profiles. Mainnet remains NO GO.

## Emergency custodian intake

`emergency_custodian_intake.py` checks a completed emergency custodian packet, the first of the three custody intakes. The packet format and collection steps are in [the emergency intake](../../../launch/custody/emergency/INTAKE.md).

```text
python3 -B tools/mainnet-preparation/emergency_custodian_intake.py EMERGENCY_INTAKE.working.json
```

It requires five custodians (or solo kits), three of five for freeze and three of five for resume, one explicit authority epoch, and SLH-DSA-SHAKE-256s keys. It checks distinct controllers and control groups (one controller and five distinct kits under `solo_kits`), ten distinct keys with SHA-256 key IDs, purpose- and epoch-bound public evidence and reviewer separation. On success it emits `authority_fragment`, the `root.emergency` records shape: the epoch and the freeze and resume key sets, each sorted by key ID with threshold 3. The upgrade and genesis checkers read the same packet through `--emergency`. It does not verify signatures, identity or independence, and it never reports production acceptance. Exit code 0 means structurally complete; 2 means incomplete or invalid.

## Upgrade custodian intake

`upgrade_custodian_intake.py` checks a completed upgrade custodian packet (E05-c). The packet format and collection steps are in [the upgrade intake](../../../launch/custody/upgrade/INTAKE.md).

```text
python3 -B tools/mainnet-preparation/upgrade_custodian_intake.py \
  UPGRADE_INTAKE.working.json --emergency EMERGENCY_INTAKE.working.json
```

It requires exactly five custodians, three of five, one explicit authority epoch, and SLH-DSA-SHAKE-256s keys (P01, 30 September 2026). It checks distinct controllers, control groups and keys, SHA-256 key IDs, purpose- and epoch-bound public evidence, reviewer separation, and that no controller, control group or key also appears in the complete emergency intake. On success it emits `authority_fragment`, the node's upgrade authority shape with keys sorted by key ID. It does not verify signatures, identity or independence, and it never reports production acceptance. Exit code 0 means structurally complete; 2 means incomplete or invalid.

Both custody checkers take the packet's `custody_model`. `independent` applies the rules above. `solo_kits` is the solo launch profile (P01, 3 October 2026): one controller in all five slots, five distinct kits as the control groups, no independence review, and the same controller and kits in the emergency, upgrade and genesis packets. Keys stay distinct across every role in both models.

## Genesis signer intake

`genesis_signer_intake.py` checks a completed genesis signer packet. The packet format and collection steps are in [the genesis signer intake](../../../launch/custody/genesis/INTAKE.md).

```text
python3 -B tools/mainnet-preparation/genesis_signer_intake.py GENESIS_INTAKE.working.json \
  --emergency EMERGENCY_INTAKE.working.json --upgrade UPGRADE_INTAKE.working.json \
  --policy-out root-genesis-policy.json
```

**What it checks:**
- The approved shape (P01, 30 September 2026): exactly five signers, three of five, the chain ID, and SLH-DSA-SHAKE-256s keys as `dytallix-root-sign keygen` writes them (`key_id`, `public_key_hex`).
- Distinct controllers, control groups and keys, with SHA-256 key IDs.
- Public evidence bound to the purpose `genesis`, with no epoch.
- Reviewer separation.
- That no controller, control group or key appears in the complete emergency or upgrade intake.

`five_holder_custody.py` checks the three packets together under the five key holders model (`"custody_model": "five_holders"`; P01, 7 October 2026: five people, one kit each, kit N holding key N of every role). Each packet must pass its own checker, and together:
- slot N is named only by its kit, `kit-holder-N` in `kit-N`, in every packet, so the five holders are the same in every role;
- the twenty keys are distinct across roles, and the emergency and upgrade keys share one authority epoch;
- no slot carries an independence review: the public disclosure stands in for it (P01, 10 October 2026, D10-Q03).

```text
python3 -B tools/mainnet-preparation/five_holder_custody.py --emergency EMERGENCY_INTAKE.working.json \
  --upgrade UPGRADE_INTAKE.working.json --genesis GENESIS_INTAKE.working.json --policy-out root-genesis-policy.json
```

It outputs the two authority fragments, the genesis signer policy and each packet's SHA-256. Like the other checkers it verifies no signature, identity or independence, and accepts nothing.

**On success:** it emits `signer_policy`, the public signer policy record. It is byte for byte what `dytallix-root-sign policy` writes from the same records, which `fixtures/genesis-signer-policy` and the signer's own test check. `--policy-out` writes it to a new file.

**What it doesn't do:** verify signatures, identity or independence. It never reports production acceptance.

Exit code 0 means structurally complete; 2 means incomplete or invalid.

## Genesis inputs (E05-d)

`resolve_genesis_inputs.py --mode rehearsal|production` writes the genesis builder's inputs from the approved values in `launch/E05_VALUES.json`, the labeled proposals in `launch/genesis/PROPOSALS.json`, and the records, with a report of each value's source. `--check` compares instead of writing. The mode must match the builder's build (production activation v1, A7). `genesis_rehearsal_records.py --out fixtures/genesis-rehearsal` writes the synthetic rehearsal records and, once the rehearsal is built, its binding-review packet (`review-records.json`, `review-bindings.json`); add `--staging` for the production-profile rehearsal on the staging chain. The builder itself is the node's `dytallix-genesis-build`; see [the genesis builder](../../docs/mainnet/e05-genesis-builder.md). `fixtures/genesis-rehearsal/` holds the development build's committed rehearsal and `fixtures/genesis-production-rehearsal/` the production build's: records, inputs, resolution, the four built files and the review packet. Review either with:

```text
F=tools/mainnet-preparation/fixtures/genesis-rehearsal
python3 -B tools/mainnet-preparation/check_bindings.py --bindings $F/review-bindings.json \
  --records $F/review-records.json --native $F/native-genesis.json \
  --application $F/application-config.json --engine $F/genesis.json --manifest $F/BUILD_MANIFEST.json
```

`network_identity.py` holds the approved network identity check (D13-Q01, `launch/genesis/IDENTITY.json`): the resolver uses it for production eligibility, `earliest --built-at T` prints the first genesis time the procedure allows after a build, and `check --genesis-time T --built-at B` checks one. See [the genesis builder](../../docs/mainnet/e05-genesis-builder.md#network-identity-and-genesis-time).

## Host values (E05)

`resolve_host_values.py` writes the host configuration generator's values from the approved per-host settings in `launch/E05_VALUES.json` and the labeled proposals in `launch/hosts/PROPOSALS.json`, with a report of each value's source; `--check` compares instead of writing. The generator itself is the engine's `dytallix-host-config`, which writes each host's engine files and binding from the public pin plan; see [host configuration](../../docs/mainnet/host-configuration.md). `fixtures/host-config-rehearsal/` holds a synthetic plan for the staging rehearsal, its resolved values and its generated bindings.

```text
python3 -B tools/mainnet-preparation/resolve_host_values.py --check \
  --values ../launch/E05_VALUES.json --proposals ../launch/hosts/PROPOSALS.json \
  --host-values tools/mainnet-preparation/fixtures/host-config-rehearsal/host-values.json \
  --resolution tools/mainnet-preparation/fixtures/host-config-rehearsal/host-values-resolution.json
```

## Host files (E05, host setup v1)

`host_files.py` writes each host's files ([host setup v1](../../docs/architecture/host-setup-v1.md)) from the release (`RELEASE_MANIFEST.json`, `BUILD_RECORD.json`), the chain's genesis and root records, `dytallix-host-config`'s output, the pin plan, the offline key summaries (`LABEL.keys.json`, schema `dytallix.host-keys.v1`: the host's public keys and the SHA-256 of each sealed secret file; the endpoint's `LABEL.channel-pin.json`), the approved E05 values and `launch/hosts/SETUP_VALUES.json`:

```text
python3 -B tools/mainnet-preparation/host_files.py --release RELEASE_DIR --chain CHAIN_DIR \
  --hosts HOSTS_DIR --plan PIN_PLAN.json --keys KEYS_DIR --out OUT_DIR
```

For each host, `OUT_DIR/LABEL/` holds every file under its installed path and `INSTALL_MANIFEST.json` (schema `dytallix.host-install.v1`): owners, modes, digests, directories, binaries and the secret files the sealed keys must provide. It runs `native-execution-policy/production_roles.py` for the role profiles, unit properties, admission and firewall table, and pins every file the supervisor reads by SHA-256. The plan's home must be `/var/lib/dytallix/node`. `fixtures/host-files-staging/` holds the configurations it writes for a synthetic three-host staging network, which the supervisor's tests parse with their own types.

`host_keys.py` is the offline key step, run on the ceremony machine with the release's key tools ([key ceremony, node keys](../../../launch/custody/KEY_CEREMONY.md#node-keys)). For each host in the pin plan it makes the peer seed, the validator key and signing state and, on the endpoint, the client channel seed and pin in a staging home; writes `LABEL.keys.json`; seals the secret files with `dytallix-root-sign seal`; and has the founder type the printed seal code back (`seal-check`). With `--backup-upload FILE` the sentry also seals the chain's backup code and the store's upload key; with `--history-upload FILE` the chain's history code is made once and sealed, with that separate upload key, on every host (monitoring v1, M4b). Its output directory holds only public files, with the pin plan's public keys filled in:

```text
python3 host_keys.py --plan PIN_PLAN.json --bin BIN_DIR --staging /tmp/staging --out KEYS_DIR
```

`host_bundle.py` packs one host's bundle from `host_files.py`'s output, the release's `bin/` and the host's sealed keys, checking every file against the install manifest with the installer's own check, and writes `LABEL.bundle.tar` (deterministic) and its `.sha256`. `host/` holds what the bundle runs on the host: `install.sh`, `stage.sh`, `switch.sh`, `restore.sh`, `monitor-settings.sh` and `wipe.sh`, wrappers for `host_install.py install|verify|stage|switch|restore|monitor-settings|wipe` (standard library only); `restore.sh COPY` restores a freshly installed host from an off-host copy (disaster recovery v1, R5), and `monitor-settings.sh FILE` installs or replaces the host's webhook URLs (monitoring v1, M4). The host files also carry the jobs the timers run: the sentry's `backup.py` and every host's `monitor.py`, which reads the node's metric files and the host's counters each minute, evaluates the approved alert rules (E05 values `monitor.*`) and pushes a heartbeat and alerts with curl ([monitoring](../../docs/operations/monitoring.md)); With the history secrets it also encrypts and uploads each finished day of its history. `test_monitor_job.py` runs it with stand-in systemctl, curl and signer. `test_host_bundle.py` runs the key step with stand-in key tools, builds bundles and installs, verifies and wipes them against a temporary root with a stand-in system; H4 installs on a real Ubuntu 24.04 runner.

`staging_chain.py` builds a throwaway production-profile staging chain and every host's bundle from a filled pin plan, the key step's output and a release build, with the release's own `dytallix-genesis-build`, `dytallix-host-config` and `dytallix-root-sign`: the production rehearsal's invented records with the plan's validators, the genesis built twice around the release manifest (the native genesis must not change), five throwaway root keys signing three of five, the host values and configuration, host files and bundles. It refuses the approved network identity and any chain ID naming mainnet or production. `staging_host.py prepare` writes a staging plan with this host as the validator and runs the key step and `staging_chain.py`; as root, `install` installs the validator's bundle, starts the node and waits for its height, and `wipe` removes it. The Host install workflow runs them on an Ubuntu 24.04 runner.

`staging_network.py` runs all three roles on one runner: `prepare` (the plan on a private bridge, the key step with a stand-in upload key for the sentry's backups, a staging snapshot interval of 10 blocks), and as root `network-up` (bridge, taps and a temporary route out), `vms-up` (two KVM virtual machines from the Ubuntu 24.04 cloud image, with cloud-init giving each its address, the guest agent and its bundle), `install-vms`, then `staging_host.py install` for the validator, then `run` (both follow the validator; the endpoint's status page; one run of each host's monitor job in its sandbox; the sentry's snapshot and backup, opened on the runner), `restore-drill` (the first restore drill, disaster recovery R6: the sentry wiped, reinstalled and restored from its copy with `restore.sh`, then matched against the validator's height and hashes, with the measured times), `kit-drill` (root kit replacement K4), `monitor-drill` (monitoring v1, M5: the sentry's monitor reports to a stand-in alerting service, and a restart loop, a full disk, a halt and a stopped monitor each alert and resolve under the approved rules) and `diagnostics`. `s3_standin.py` is the stand-in store: loopback only, signed PUTs only, never a real store. `alert_standin.py` is the stand-in alerting service: loopback only, it records heartbeats, alerts and escalations and alerts on a heartbeat missing for 3 minutes, never a real service.

## Decision copies (E05)

Each gate in `launch/LAUNCH_GATES.json` lists its decision dependencies as copies of questions in `launch/MAINNET_DECISION_REGISTER.json` (`source_ref`), and its `decision_counts` copies the register's `question_counts`. The register is authoritative. After recording an approval there, run:

```text
python3 -B tools/mainnet-preparation/decision_copies.py --write
```

It copies each question's kind, status, text, blocking output, assignee, reviewer and approval record into every copy, and the counts. It never changes a gate's status or the register. Without `--write` it reports each difference and exits 1; `test_decision_copies.py` fails the same way, so a register change without its copies fails CI.

## Tests

Run `python3 -B -m unittest discover -s tools/mainnet-preparation -p 'test_*.py'`.

The fixtures are synthetic. They reuse public keys and selected local parameters from prior development evidence. They contain no production private key and establish no production operator, beneficiary, amount, vesting schedule, network address, or approval. Runtime startup and heavy builds are outside this test scope.
