# Disaster recovery v1 (F19)

Engineering task E05, artifact F19. How the chain's history and state
survive losing hosts, under the solo launch profile: one validator, one
sentry (the archive) and one endpoint, reached only by console. The
approved objectives fix the rules
([operations objectives](../../../launch/operations/OBJECTIVES.md), D12-Q02):
no committed block is ever lost (RPO zero), a restore never rolls history
back, daily snapshots are copied encrypted with AES-256 to a second
provider, and a monthly copy goes to the founder's own drive. This
document is the design; the tools and the runbook follow (see
[Steps](#steps)).

## Decisions (P01, 7 October 2026)

[Approval](../../../launch/approvals/P01_E05_DISASTER_RECOVERY_2026-10-07.json).

- **The sentry pushes the copies.** Nothing can pull blocks or snapshots
  from the hosts: the client channel serves only `/abci_query` and
  transaction submission, and the hosts take no remote login. So the
  sentry, which holds the archive and the daily snapshots, encrypts its
  newest snapshot and uploads it to object storage at a second provider,
  with an upload key that can add objects but never read or delete them.
  The founder downloads the latest copy to their own drive each month.
- **The validator and the sentry run at different hosting providers.**
  With one validator, losing the validator and the sentry together loses
  every block after the last copy, and the chain could not resume without
  rolling history back. At two providers, no single provider or account
  failure takes both, so each committed block survives on the other host
  (a constraint on the provider choice, D12-Q03).

## What is copied

The sentry runs with `block_history: archive`, so its application keeps
every block record. A snapshot of height H holds the committed state in
chunks plus the block records the node retains
([state sync v1](state-sync-v1.md)): on the sentry, every record since
genesis. One snapshot is therefore both the state and the application's
full history. Each day (`snapshots.interval_blocks` 17,280, about a day at
5-second blocks) the newest complete snapshot is copied.

A state sync restore also needs the engine's signed headers from the
snapshot's height (light blocks from H to H+2). The sentry's daily copy
includes them where it can export them while running; otherwise the
validator or a surviving node supplies them at restore time (an open item,
below).

## The copy

- **Format** (`dytallix.backup.v1`, built in R2): a header line naming the
  chain, the height, a random 32-byte salt and the chunk size, then the
  snapshot directory as a deterministic tar (sorted; directories and
  regular files only; root-owned, time zero) in AES-256-GCM chunks of
  1 MiB. Each chunk's additional data is the header's SHA-256, its index and
  a final flag, so a truncated, reordered, extended or altered copy, or one
  with another header, is refused; the last chunk is always final.
- **Key:** SHAKE256 of a domain, the chain, the height, the salt and the
  chain's 256-bit **backup code**: random, printed once as a checked paper
  line (`dytallix-backup-CHAIN` and seventeen groups, like the seal codes),
  written twice and kept with two different kits' papers. The salt gives
  every copy its own key, so a chunk index is a safe nonce. The tool is the
  release's own Go signer (standard library AES-GCM), so nothing classical
  enters the node's stack.
- **Commands** (`dytallix-root-sign`): `backup-code` makes the code file the
  sentry's bundle will seal and prints the paper line; `backup-seal` packs
  and encrypts a snapshot directory (never overwriting, no partial copy
  left on failure); `backup-check` opens a copy with the typed code and
  checks every entry without writing; `backup-open` checks the whole copy,
  then extracts it into a new owner-only directory.
- **Name:** the chain, the height and the SHA-256 of the copy, which
  `backup-seal` prints.
- **Upload:** the host's own `curl` with AWS Signature V4 to an
  S3-compatible bucket, over TLS. TLS is transport only, as for the bundle
  download: the copy is already encrypted and authenticated, and no TLS
  library enters a release binary (G35).
- **Upload key:** write-only (it can add objects, not list, read or delete
  them), sealed into the sentry's bundle with its node keys, so it reaches
  the host the same way and is never typed or stored in the clear
  elsewhere. A stolen key can only add objects.
- **Job** (built in R3): `dytallix-backup.timer` (hourly, randomized by up
  to 15 minutes, persistent) and the oneshot `dytallix-backup.service`,
  separate from the node's unit and its AppArmor roles. It runs
  `/etc/dytallix/RELEASE/backup.py` as root with only
  `CAP_DAC_READ_SEARCH` (to read the node's snapshots), a read-only system
  and `/var/lib/dytallix-backup` as its only writable path. When the newest
  published snapshot is above the last height uploaded, it runs
  `backup-seal` into its scratch directory, uploads with `curl --config -`
  (the upload key on curl's standard input, never on a command line),
  records the height and removes the scratch copy. It never touches the
  node's home or keys.
- **Where the secrets live:** the backup code and upload key are sealed as
  `backup/code` and `backup/upload.json` with the sentry's node keys. The
  installer unseals into a root-only directory and places them in
  `/etc/dytallix-backup` (root, 0400), outside the node home and its
  profiles; `verify` checks them and `wipe` removes them.
- **Retention at the provider:** the bucket keeps every copy (no deletion
  by the upload key); the founder prunes old copies by hand, keeping at
  least one per month.

## Restore

- **Sentry lost** (validator alive): rebuild the sentry from its bundle,
  join by state sync from the newest off-host copy (decrypted with the
  backup code) and catch up from the validator. Its archive history comes
  back with the snapshot's block records.
- **Validator lost:** [validator recovery](../operations/validator-recovery.md)
  (F17). The sentry holds every block.
- **Endpoint lost:** rebuild it from its bundle and state sync.
- **Validator and sentry lost together:** only possible if both providers
  fail at once. Restore from the newest off-host copy; blocks after it are
  lost, and the chain resumes only after an incident decision. The
  two-provider rule is what makes this the last case.

Every restore is drilled quarterly on staging, with its measured times.

## Open items

- **A backup run on a real host:** H4's staging network job installs the
  sentry in a VM, runs its backup job against a stand-in store
  (`s3_standin.py`) and opens the copy on the runner; its first green run
  closes this item.
- **Light blocks from a running sentry:** `dytallix-light-export` reads the
  engine's stores and asks for a stopped node or a copy of its home. The
  copy needs an export that works while the node runs, or the light blocks
  come from another node at restore time.
- **The second provider and the hosting providers** (D12-Q03), and the
  storage account. A free tier must hold the copies: the snapshot grows with
  history.
- **Runbook:** `node/docs/operations/disaster-recovery.md`, once the tools
  exist.

## Steps

- **R1.** This design and the decisions.
- **R2.** The encryption and restore tools, with tests. Built:
  `dytallix-root-sign backup-code`, `backup-seal`, `backup-check` and
  `backup-open` over `root-authorization/backup.go`.
- **R3.** The upload key in the sealed bundle, the backup unit and timer in
  the host files and installer; H4 runs one backup to a local stand-in
  store. Built except the H4 run: `host_keys.py --backup-upload`,
  `backup/` sealed files, `host_files.py`'s backup unit, timer and
  `backup.py`, the installer's placement, verify, stage, switch and wipe,
  with tests (`test_backup_job.py`, `test_host_bundle.py`).
- **R4.** The runbook and the first restore drill on staging.
