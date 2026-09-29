# Restart on a fixed release

Runbook v1 ([index](README.md); design in
[restart v1](../architecture/restart-v1.md)). The following are unset
inputs (D14-Q03, E05, P02):
- the handover custodians;
- their signing procedure;
- the incident channel;
- the release process for the fixed release.

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
   - It writes `restart-unsigned.json` and `restart-artifact.bin`, the exact
     bytes to sign.
   - It prints the sequence, the halted height and the artifact's SHA-512.
4. **Compare across operators.** Several operators run step 3 on their own
   nodes. The payloads must be identical; a difference means the nodes
   diverged, so follow [fork.md](fork.md).

## Signing

5. **The handover custodians sign.** At least the handover threshold of
   custodians sign `restart-artifact.bin` under the root `upgrade` action,
   with the printed sequence and the height window H..H. Custody is E05 and
   P02; the repository's fixture signer is test-only.
6. **Assemble the file.** Add each signature to `signatures` in
   `restart-unsigned.json` as `{"key_id": ..., "signature_hex": ...}`,
   sorted by key ID. The file may stay formatted; only the payload is
   signed.

## Restart

7. **Distribute** the authorization and the fixed release through the
   approved channel.
8. **Pin and install.** Each operator installs the fixed release's
   artifacts. They pin the authorization in the service configuration as
   `restart_authorization`: path, SHA-256 and a byte bound of at most
   256 KiB.
9. **Start the service.**
   - The supervisor's preflight verifies the authorization with the root
     helper and selects the fixed release.
   - The application runs block H on it.
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
