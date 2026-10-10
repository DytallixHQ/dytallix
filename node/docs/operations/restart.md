# Restart on a fixed release

Runbook v1 ([index](README.md); design in
[restart v1](../architecture/restart-v1.md)). The signers are the upgrade
keys of the five key kits ([key ceremony](../../../launch/custody/KEY_CEREMONY.md));
the fixed release is built and frozen as in [release](../../../release/README.md).
Still unset (D14-Q03, E05, P02): the incident channel.

Use this procedure when the chain is halted at height H on every validator
and the fix is new code. It covers two cases:
- a decided block H fails to execute;
- no block H can be proposed.

Enter it from [halt.md](halt.md) step 8, [supply-mismatch.md](supply-mismatch.md)
case 1 or [upgrade-failure.md](upgrade-failure.md) case 6.

A restart authorization switches the active release at H and nothing else:
- block H's effects are whatever the fixed release computes;
- history is not rewritten;
- a freeze in force stays in force.

The chain resumes only if more than two thirds of the voting power runs
the fixed release with the authorization.

## Before signing

1. **Confirm the halt.** Every validator stops at the same height H.
   Record whether block H was decided: a trusted peer or gateway serves
   `/block?height=H`, or the engine's block store holds it. When it was
   decided, record its hash.
2. **Build the fixed release** through the release process. Record its
   manifest digest (SHA-512). The fixed release must execute every block
   before H exactly as the committed release did. The authorization cannot
   prove this, so release review must.
3. **Build the unsigned authorization** on a stopped node:
   ```sh
   dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db HOME/appdb \
     --restart-target RELEASE_SHA512 --evidence INCIDENT_SHA256 \
     [--halted-block-hash BLOCK_H_HASH] --restart-output DIR
   ```
   - The tool first runs the stopped-node checks. They must pass, at
     height H−1.
   - It reads the committed checkpoint, the active release, the next
     handover sequence, the emergency history and any pending admission.
   - It writes `restart-unsigned.json`, `restart-artifact.bin`, the exact
     bytes to sign, and `restart-request.json`, the signing request
     (`dytallix.control-request.v1`) with the upgrade keys in force there.
   - It prints the sequence, the halted height and the artifact's SHA-512.
4. **Compare across nodes.** Run step 3 on at least two nodes (under the
   solo launch profile, the validator and the sentry). The payloads must be identical; a difference means the nodes
   diverged, so follow [fork.md](fork.md).

## Signing

5. **The upgrade keys sign.** At least the handover threshold of upgrade
   keys (three key kits under the solo launch profile) sign the request
   offline, as any root control ([control signing](../mainnet/control-signing.md)):
   ```sh
   dytallix-root-sign show-control -request restart-request.json
   dytallix-root-sign sign-control -request restart-request.json \
     -private-key /mnt/kit/kit-N-upgrade.key -public-key /mnt/public/kit-N-upgrade.json \
     -operation restart -sequence S -out signatures/kit-N.sig.json
   ```
   The signer reads what it shows from the artifact: the halted height H
   (the window is H..H, under the root `upgrade` action), the source and
   target releases, block H's hash or that it was never decided, and the
   bound receipts and evidence. Check them against steps 1 to 4. It refuses
   a request whose halted height is not the committed head plus one.
6. **Assemble the file** on the stopped node:
   ```sh
   dytallix-state-check --config APPLICATION_CONFIG --genesis NATIVE_GENESIS --db HOME/appdb \
     --restart-assemble restart-request.json --restart-signatures signatures --restart-output DIR
   ```
   It counts only the upgrade keys in force on this node (never the
   request's copy), needs the threshold to the maximum, sorts the signatures
   and checks the result with the node's decoder. It writes
   `restart-authorization.json` and prints its SHA-256 and size for the pin
   (step 8). It does not verify the signatures; the application does at
   startup.

## Restart

7. **Distribute** the authorization and the fixed release through the
   approved channel.
8. **Pin and install.** Each operator installs the fixed release's
   artifacts. They pin the authorization in the service configuration as
   `restart_authorization`: path, SHA-256 and a byte bound of at most
   256 KiB.
9. **Start the service.**
   - The supervisor's preflight checks the authorization against the
     committed state and selects the fixed release; it never runs the root
     helper ([supervisor root preflight](../architecture/supervisor-root-preflight.md)).
   - The application verifies the authorization's signatures with the root
     helper before it opens state, then runs block H on the fixed release.
   - A refused authorization stops startup with exit class `release` (14).
10. **Verify.** Check that:
    - the height rises past H on every validator;
    - the application status shows the fixed release as
      `release_handover.active_release_sha512`;
    - `next_sequence` has advanced by one.
11. **Remove the pin** once the chain has resumed. A committed authorization
    is ignored at later startups, and any other one is refused.

## Later

- **Block sync.** A node that block-syncs across H runs the committed
  release up to H−1. It then stops, and starts on the fixed release with
  the authorization.
- **State sync.** A node that state-syncs past H needs neither.
- **The old release** is refused from H on (`release`).

## Do not

- Sign an authorization whose payload operators have not independently
  reproduced (step 4).
- Edit the payload of a signed file, which invalidates every signature.
- Start the fixed release without the authorization, or the old release
  after H.
