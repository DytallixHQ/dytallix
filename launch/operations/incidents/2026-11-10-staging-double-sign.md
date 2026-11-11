# Incident: a double sign on staging during the validator recovery drill (10 November 2026)

**Status:** closed on 11 November 2026 ([closeout decisions](../../approvals/P01_T06_STAGING_DOUBLE_SIGN_CLOSEOUT_2026-11-11.json)). Staging only. On mainnet the same sequence would have been severity 1.

## What happened

The validator recovery drill (T9) ran on the real staging hosts of `dytallix-staging-2`: one validator, `validator-1`, and one sentry, `sentry-1` ([approval](../../approvals/P01_T06_VALIDATOR_RECOVERY_STAGING_2026-11-06.json)). It followed the fence-first procedure ([validator recovery](../../../node/docs/operations/validator-recovery.md)) as it stood then. The sentry failed to start. The replacement validator then had no peer, began a new chain from height 1, and signed heights 1 to 1,401 with the key that had signed heights 1 to 173,074 of the real chain. That is equivocation.

All times are UTC, on 10 November unless noted. The drill's instants come from the founder's machine.

| Time | Event |
|---|---|
| 17:52 | Provider snapshot of `validator-1` |
| 17:54:27 | Declared |
| 18:00:29 | Fenced: the provider reinstalled `validator-1` on the same server and address |
| | Halt read twice on the sentry: H = 173,074 |
| 18:03:15 | The sentry's consensus log set aside |
| 18:05:05 | The replacement installed from its sealed bundle (the staging release, with the seal code) |
| 18:05:50 | The sentry started |
| 18:05:51 | The validator started, one second later; nothing checked that the sentry was up |
| 18:06:55 | The sentry's supervisor exited (`BOUNDED_STARTUP_FAILURE`), and systemd did not restart it |
| 18:06 to 19:40 | The replacement, alone and the only validator, signed heights 1 to 1,401 of a fork |
| 19:40:16 | The replacement stopped by hand, and its unit disabled |
| 19:54 | Provider snapshot of `sentry-1` (stopped at 173,074) |
| 11 Nov, 02:17 | Containment checked at both consoles: nothing running, the validator disabled, its signing state at 1,401 |
| 11 Nov, 02:19 | Provider snapshot of the replacement's forked disk, kept as evidence |
| 11 Nov, 02:21 | `validator-1` reinstalled to the bare operating system; no node is left (checked at 02:23) |

## Impact

- **Staging only, with rehearsal identities.** No mainnet key or host was involved.
- **The fork stayed on the replacement.** It had no peer, so no vote, block or evidence of the fork reached another node.
- **The real staging chain is intact** to 173,074 on the sentry. `dytallix-staging-2` has made no block since the fence and will not be restarted.
- **On mainnet this ends the chain.** There is one validator, and its first double sign removes it for good ([penalties v1](../../../node/docs/architecture/penalties-v1.md)).

## Causes

1. **No signing floor.** The procedure's rule "never restore signing state from a backup" meant the replacement started from fresh signing state, so it could sign any height, including heights below H.
2. **No start gate.** The procedure started the validator right after the sentry, without first confirming that the sentry was up and still at H.
3. **The sentry could not start at the halt height.** Opening the application at the archive height 173,074 took longer than the supervisor's 60-second startup wait for the application's `info` answer, so the supervisor failed the start ([measurements](https://github.com/DytallixHQ/dytallix/pull/135)). With no peers, its engine was also never "caught up", so the readiness checks could not pass.
4. **An only validator does not block-sync.** The engine skips block sync when its own key is the only validator. It catches up through consensus from its last height, which for an empty store is 1. Alone, that means making new blocks from height 1.
5. **Nothing raised an alarm.** Once the validator started, only the founder's poll of the sentry's status page was watching, and with the sentry down that page served nothing. The fork was found by hand.

## Fixes

| Change | What it does |
|---|---|
| [#134](https://github.com/DytallixHQ/dytallix/pull/134) | **Signing guard:** `host_install.py guard H` sets the replacement's signing state to H at its highest round and step before its first start. The engine then refuses to sign any height up to H, so a replacement with no peer stalls at height 1 and signs nothing. **Start gate:** `recovery.py hold` reads the sentry's engine through its agent, past its startup budget, and gives GO only while it is up and at H. `first-block` stops on a sentry below H. The runbook gains both steps and the stop rule. |
| [#136](https://github.com/DytallixHQ/dytallix/pull/136), [#137](https://github.com/DytallixHQ/dytallix/pull/137) | A startup checkpoint, so a node opens quickly at its archive height; one startup check for each start; a sentry or endpoint with no peers counts as ready. |
| [#138](https://github.com/DytallixHQ/dytallix/pull/138) | The HTTP adapter and client channel readiness follow the same no-peer rule, so the CI drill's sentry starts while the validator is fenced. |

Bundles sealed before #134 carry the old `host_install.py`, which has no guard. The procedure stops on them, so every staging bundle is sealed again.

## Closeout (P01, 11 November 2026)

- **Evidence kept:** three provider snapshots, retired and never deleted:
  - `validator-1` before the drill;
  - `sentry-1` after the incident;
  - the replacement's forked disk.

  The drill's run folder stays on the founder's machine.
- **The fork removed:** `validator-1` is reinstalled to the bare operating system, so the forked signing state can never start again.
- **`dytallix-staging-2` retired.** Staging comes back as `dytallix-staging-3`:
  - a release carrying #134, #136, #137 and #138;
  - bundles sealed again;
  - a new genesis;
  - both hosts reinstalled.

  That install and each host's reinstall come with their own approval.
- **The drill runs again on `dytallix-staging-3`,** with the guard and the start gate, under a new approval. The 6 November approval excluded a second run.

## Still open

These items are carried forward; none of them blocks the closeout:
- **The startup budget,** including the 60-second cap.
- **The startup check's scope.**
- **The sentry's archive mode.**
- **4 GB staging hosts.**
- **`Restart=no` on the node unit.** It is why systemd did not restart the sentry.
- **Catch-up time through consensus.** A replacement of the only validator catches up through consensus rather than block sync. Whether a long chain still fits the 6-hour catch-up objective is not yet measured.
- **Watching the sentry after the start.** Once the validator starts, only `first-block` watches the sentry.
