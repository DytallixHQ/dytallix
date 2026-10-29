# Starting a later release

Status: **proposed** (P01 decision pending, 28 October 2026). Found while
mapping the upgrade drill (simulation plan, T8): a production node cannot
start on a release the chain has committed by handover. Production
handover is "not yet qualified" ([upgrade or handover
failure](../operations/upgrade-failure.md)); this is the reason it cannot
qualify.

## The gap

The chain's first release (RC1) is fixed at genesis, and every later
release is approved on chain by a handover signed three of five
([release approval](../../../launch/approvals/P01_E06_RELEASE_2026-10-05.json)).
After a handover to RC2:

- **The supervisor already does the right thing.** Its preflight selects
  the committed active release, RC2
  (`consensus_settlement.rs`, `preflight_prepared_release`, the
  `handover_state(..).active_release_sha512()` branch), checks RC2's
  catalog against it (`runtime_candidate_v2::verify_catalog`) and admits
  the application with RC2's SHA-512 as its owner context
  (`native-supervisor/src/processes.rs`, `--release-manifest-sha512`).
- **The root configuration stays on RC1, and must.** The root genesis
  signatures cover the genesis release's digest (`root_genesis::bundle`),
  and `root_checks` requires the consensus configuration's
  `emergency.release_sha512`, which is genesis-bound, to equal it.
- **The application then refuses RC2.** `consensus_stdio`'s production root
  branch requires the root configuration's release to equal the owner
  context ("Root authorization release differs from owner admission"),
  which holds only for RC1.
- **The host files cannot even express RC2.** `host_files.py` points the
  root configuration and the runtime candidate at one file,
  `{etc}/release-manifest.json`, and the root configuration checks that
  file against its pinned SHA-512 (`root_genesis.rs`,
  `verified_artifact_digest`). An RC2 bundle has RC2's manifest there, so
  either the root check or the candidate is wrong.

The signed handover process tests pass because they launch both releases
through the development root with the genesis release as the owner context,
which the production supervisor never does.

## Proposal

1. **The owner context names the release that runs.** With a runtime
   candidate (always present with a handover policy:
   `ensure_active_runtime` requires one), the owner context must equal the
   candidate's verified manifest SHA-512, and `ensure_active_runtime`
   already refuses any candidate other than the committed active release
   (the genesis release before any handover). The check therefore moves
   from `consensus_stdio`, before the candidate is verified, into
   `ConsensusApplication::open_with_root_runtime`, after it. Without a
   runtime candidate, the owner context must still equal the root
   configuration's release, as now. The two development branches follow
   the same rule, and the process tests launch each release with its own
   context, as the supervisor does.
2. **The root configuration keeps the genesis release in its own file.**
   Every bundle carries `{etc}/genesis-release-manifest.json`, the genesis
   release's manifest, and the root configuration names it with its
   SHA-512; the runtime candidate and the supervisor's catalog keep
   `{etc}/release-manifest.json`, the bundle's own release. For a genesis
   bundle the two files are identical. `host_files.generate` takes the
   genesis manifest as an input, defaulting to the bundle's own for a new
   chain, so later releases of an existing chain can be generated.
3. **Unchanged:** the root genesis signatures and their verifier, the
   emergency policy's release, the handover and upgrade controls, the
   supervisor, the catalog checks, and the rule that a release may run
   only once the chain has committed it (`ensure_active_runtime`). A
   later release must keep the root verifier (`dytallix-root-verify`)
   byte for byte: the root configuration pins its SHA-256 and the catalog
   binds it to the genesis bootstrap verifier role.

## Alternatives not taken

- **The supervisor passes the genesis release as the owner context**, as the
  development tests do: the owner context would no longer name the binary
  set the supervisor verified and launched.
- **New root signatures per release:** the root genesis is fixed at genesis;
  a release changes only by the on-chain handover.

## Tests and proof

- The owner-context rule as a unit test: the running candidate's release
  is admitted, any other context is refused, and without a candidate only
  the root release is admitted.
- The process tests launch each release with its own owner context.
- `host_files.py`: a later-release bundle pins the genesis manifest for the
  root and its own manifest for the candidate; a genesis bundle is
  unchanged apart from the extra file.
- End to end: the upgrade drill (T8) in the CI staging network, RC1 to RC2
  with the receipt-index migration, staged and switched on both hosts.

## Scope

Consensus code (`consensus_stdio`, the application's open path) and the
host files change, so the change is in the audit candidate and the E01
inventory is regenerated. A restart onto a new release (restart v1) starts
through the same check and is covered by the same rule.
