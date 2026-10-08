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

[Restore approval](../../../launch/approvals/P01_E05_DISASTER_RECOVERY_RESTORE_2026-10-07.json):

- **A rebuilt host restores on its own.** Only the sentry writes and serves
  snapshots, so a rebuilt sentry has no peer to state-sync from. Instead,
  after installing from its bundle and before its first start, the host
  opens the newest off-host copy and restores from it locally, then catches
  up from the validator. No other host serves the copy or changes, and the
  same path rebuilds the validator if the validator and the sentry are lost
  together.
- **Light blocks are written with each snapshot.** The sentry's engine
  writes the light blocks for each snapshot height, so every copy is
  self-contained and no node is stopped to export them (with one validator,
  stopping it halts the chain).

[Restore age and the validator](../../../launch/approvals/P01_E05_RESTORE_2026-10-08.json)
(8 October 2026): a restore accepts a copy up to 13 days old, and on the
validator only with `--accept-history-loss` after the incident decision.

## What is copied

The sentry runs with `block_history: archive`, so its application keeps
every block record. A snapshot of height H holds the committed state in
chunks plus the block records the node retains
([state sync v1](state-sync-v1.md)): on the sentry, every record since
genesis. One snapshot is therefore both the state and the application's
full history. Each day (`snapshots.interval_blocks` 17,280, about a day at
5-second blocks) the newest complete snapshot is copied.

A restore also needs the engine's signed headers from the snapshot's
height: light blocks from H to H+2, since header H+1 carries the
application hash of H and H+2 commits it. The sentry's engine writes them
once H+2 commits, in the export format (`{height:020}.block` and
`.params`), to `/var/lib/dytallix/snapshot-light-blocks/{H:020}` (staged,
then renamed), and keeps as many as the application keeps snapshots
(`dytallix-pqc-engine start --snapshot-light-blocks DIR --snapshot-interval N
--snapshot-keep K`, from the supervisor's `snapshots` settings). The backup
job takes the newest height with both and copies them together.

## The copy

- **Format** (`dytallix.backup.v1`, built in R2): a header line naming the
  chain, the height, a random 32-byte salt and the chunk size, then the
  snapshot directory, with its six light block files under `light-blocks/`,
  as a deterministic tar (sorted; directories and
  regular files only; root-owned, time zero) in AES-256-GCM chunks of
  1 MiB. Each chunk's additional data is the header's SHA-256, its index and
  a final flag, so a truncated, reordered, extended or altered copy, or one
  with another header, is refused; the last chunk is always final.
- **Key:** SHAKE256 of a domain, the chain, the height, the salt and the
  chain's 256-bit **backup code**: random, printed once as a checked paper
  line (`dytallix-backup-CHAIN` and seventeen groups, like the seal codes),
  written twice and kept by the founder in two separate places, like the
  seal codes. The salt gives
  every copy its own key, so a chunk index is a safe nonce. The tool is the
  release's own Go signer (standard library AES-GCM), so nothing classical
  enters the node's stack.
- **Commands** (`dytallix-root-sign`): `backup-code` makes the code file the
  sentry's bundle will seal and prints the paper line; `backup-seal` packs
  and encrypts a snapshot directory with its light blocks (`-light-blocks`,
  required: exactly heights H to H+2; never overwriting, no partial copy
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
  height with a published snapshot and its light blocks is above the last
  height uploaded, it runs
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

A rebuilt host restores from the newest copy, as root, after `install.sh`
and before the node's first start, with `restore.sh COPY` from its bundle
(built in R5):

1. **Fetch** the copy to the host (it is encrypted and authenticated, so
   the download is transport only, like the bundle's).
2. **Checks.** The installed release is the bundle's, the node is stopped
   and holds no chain state (no engine stores, an empty application
   database), and on the validator the operator added
   `--accept-history-loss` (P01, 8 October 2026).
3. **Open** it with the backup code typed from paper (`backup-open`): the
   whole copy is checked before anything is extracted, into
   `/var/lib/dytallix/restore/copy`. It is the chain's public state, so the
   node may read it and only root write it; every role reads that tree.
4. **One restore start.** A drop-in under `/run` adds
   `--restore-snapshot DIR` to the supervisor for one start; the supervisor
   passes it to the engine. The node then restores the way a state sync
   joins, with the chunks read from the copy instead of from peers:
   - the engine's light client trusts the copy's header at H (the copy is
     authenticated by the backup code) and verifies H+1 and H+2 with
     ML-DSA-65, giving the trusted application hash;
   - the snapshot is offered to the application, which checks every
     chunk's hash, rebuilds the tree against the state digest, the head
     against the trusted application hash, and runs the full startup check
     ([state sync v1](state-sync-v1.md), C3); a chunk it asks for again is
     read once more, and a third request fails the restore;
   - the engine checks the application's height and hash, sets its stores
     at H and switches to block sync.
   A failure fails the start; the node then holds a partial state, and the
   host is wiped and installed again.
5. **Catch up.** The node fetches blocks from H+1 from its peers and checks
   each against its own application. Once the supervisor reports it
   started at or above H, `restore.sh` removes the drop-in and the opened
   copy; later starts are ordinary.

A restore trusts the copy's header for the evidence age less one day:
13 days with the approved 14 (P01, 8 October 2026), just under the bound
the engine requires of any trust period. Peer state sync joins keep their
approved 168 hours. The validator keeps its blocks for at least the
evidence horizon (241,920 blocks, about 14 days), and the newest copy is
at most about a day old, so a lost sentry must be restored within about 12
days of its loss.

- **Sentry lost** (validator alive): rebuild and restore as above. Its
  archive history comes back with the snapshot's block records.
- **Validator lost:** [validator recovery](../operations/validator-recovery.md)
  (F17). The sentry holds every block.
- **Endpoint lost:** rebuild and restore as above.
- **Validator and sentry lost together:** only possible if both providers
  fail at once. Rebuild the validator and restore it from the newest copy;
  blocks after it are lost, and the chain resumes only after an incident
  decision. The two-provider rule is what makes this the last case.

Every restore is drilled quarterly on staging, with its measured times.

## Open items

- **A backup run on a real host:** closed. H4's staging network job
  installs the sentry in a VM, runs its backup job against a stand-in store
  (`s3_standin.py`) and opens the copy on the runner; its first green run
  copied and opened the snapshot of height 20 (8 October 2026).
- **The second provider and the hosting providers** (D12-Q03), and the
  storage account. A free tier must hold the copies: the snapshot grows with
  history.
- **Runbook:** written ([disaster recovery](../operations/disaster-recovery.md)).

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
  with tests (`test_backup_job.py`, `test_host_bundle.py`). The H4 run
  passed.
- **R4.** The sentry's engine writes the light blocks for each snapshot
  height, and the backup copies them with the snapshot. Built:
  `lightblocks.SnapshotWriter` and the engine's `--snapshot-light-blocks`
  flags, the supervisor's `snapshots.light_blocks`, the host files'
  directory, `backup-seal -light-blocks` and the backup job, with tests;
  H4's network run checks that the copy holds the sentry's light blocks.
- **R5.** The restore: the application's snapshot import through its
  restore checks, the engine's bootstrap from verified light blocks, and
  the bundle's restore step, with tests. Built: the fork's
  `statesync.RestoreLocal` and `node.LocalSnapshot` (the state sync's own
  offer, apply and verify, with a local chunk source),
  `dytallix-pqc-engine start --restore-snapshot DIR`, the supervisor's
  `--restore-snapshot DIR`, the hosts' restore directory, and
  `restore.sh` over `host_install.py restore`.
- **R6.** The runbook and the first restore drill on staging: H4 wipes the
  sentry's VM, reinstalls it from its bundle, restores it from the stand-in
  store's copy, and checks that it catches up from the validator with the
  chain's application hash. The drill records its measured times. Built:
  [the runbook](../operations/disaster-recovery.md) and
  `staging_network.py restore-drill`, a step of the Host install workflow's
  network job.
