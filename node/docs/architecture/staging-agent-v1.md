# Staging agent v1

Simulation tool T2, first part ([simulation plan](../mainnet/launch-rehearsals.md#the-staging-agent)).
The production hosts are console-only. While they serve as staging, each
runs a small agent that carries out the founder's signed commands, so the
statistical campaign and the seven runs can drive them without a login.
It is built and proven in the CI staging network (`staging_network.py
agent-drill`). Its open choices were decided by P01 on 14 October 2026
([approval](../../../launch/approvals/P01_T06_STAGING_AGENT_2026-10-14.json));
see [Decided by P01](#decided-by-p01). The fault commands are T5's.

## What it is

- **Two files,** never in a release or a host bundle:
  `node/tools/mainnet-preparation/staging-agent/agent.py` (standard
  library, Ubuntu 24.04's python3) and its `ALLOWLIST.json`. They are
  outside the host layout, so no bundle manifest can name them.
- **On the host** after the node's install: `/opt/dytallix-agent/` (the two
  files, root, read-only), `/etc/dytallix-agent/settings.json` (root, 0400),
  `/var/lib/dytallix-agent/` (state, outbox, scratch; root, 0700) and the
  `dytallix-agent.{service,timer}` units, a oneshot every minute.
- **Removed** by `wipe.sh` (`host_install.py wipe`) with the node, or by
  `agent.py remove`. `install.sh` refuses a host that still has any of its
  paths, so a production install cannot happen beside it.
- **Founder side:** `staging_command.py` makes the key, the host settings
  and the signed command files, and packages the agent for the console.

## The staging command key

An ML-DSA-65 key in a keystore of its own on the founder's machine, made
and used only through the release's CLI: `dytallix wallet create --name
staging-command` and `dytallix crypto sign`. No new cryptography: the agent
checks signatures with the same release's `dytallix crypto verify`, under
the public key pinned in its settings (never one from a command file). It
is a staging key only; it is not a kit key or a wallet that holds tokens.

## Commands

A command file is JSON, at most 64 KiB:

```json
{"body": {"schema": "dytallix.staging-command.v1", "chain_id": "...", "host": "sentry-1",
          "sequence": 7, "not_after": 1791900000, "command": "journal", "args": {"lines": 200}},
 "signature": "<ML-DSA-65 signature, lowercase hex>"}
```

The key signs `dytallix-staging-command-v1:` followed by the SHA-256 (hex)
of the body's canonical JSON (sorted keys, no spaces, ASCII). The founder
uploads it to the command store as `PREFIX/CHAIN/HOST/SEQUENCE.json`, the
sequence in ten digits.

Each poll fetches the next sequence's object with the host's curl (AWS
Signature V4, a read key for the command prefix, outbound only; TLS is only
transport). A 404 ends the poll. For an object it:

1. **spends the slot** (the next sequence is written before anything runs,
   so a command runs at most once, even across a crash);
2. **checks** the shape (strict JSON: no duplicate keys, floats or extra
   fields), then the signature, then the binding: this chain, this host,
   this sequence, not expired and valid for at most 24 hours, then the
   allowlist and each argument's range;
3. **runs** an allowed command's fixed steps, each an argument list with no
   shell, with a minimal environment and a per-command timeout; integer
   arguments fill fixed positions; output is kept to 64 KiB a stream;
4. **records** the outcome (`DONE`, `FAILED`, `REFUSED` with its reason, or
   `NOT_BUILT` for a stub) with the command file's SHA-256, encrypts it like
   the metric history (`dytallix-root-sign history-seal` under the chain's
   history code, bound to the host and day) and uploads it with the history
   upload key as `PREFIX/CHAIN/agent/HOST/SEQUENCE-SHA256.bin`. A failed
   upload waits in the outbox for the next poll.

It handles up to 16 commands a poll. Its state names the chain; a state for
another chain stops it (a new chain is a new install, never sequence 1
again). The founder opens results with `dytallix-root-sign history-open`.

## The allowlist

| Command | Steps |
|---|---|
| `status` | the unit's state, the operator status, the application metrics, disk, memory, clock sync |
| `journal` (`lines` 1 to 5,000) | the node's newest journal lines |
| `start`, `stop` | the node's unit |
| `monitor-run`, `backup-run` | one run of the monitor or the sentry's backup job |
| `fault-kill` | SIGKILL to the node's processes (T5's process kill) |
| `fault-netem`, `fault-partition`, `fault-clear` | T5's latency and loss (`tc netem`), its self-expiring p2p partition (`inet dytallix_fault`), and their removal |

Nothing that needs a code typed from paper (install, restore, stage,
switch, wipe, monitor settings) is on it: those stay at the console.

## Installing

At the console, after `install.sh`, from the archive `staging_command.py
package` writes (its SHA-256 typed, like a bundle's) and the host's settings
file from the founder's own media, as for the monitor settings:

```text
tar -xf staging-agent.tar && python3 dytallix-agent/agent.py install --settings FILE
```

It refuses a host with no node, settings for another host or chain, a
release without the CLI, or a host without the history secrets.

## Threat model

- **The command store.** Whoever can write the command prefix cannot forge
  a signature. They can withhold commands, or spend slots by planting
  objects that are refused (each refusal is recorded); the founder then uses
  the next sequence. A signed command replayed at another sequence, host or
  chain, or after its expiry, is refused; at its own slot it has already
  run.
- **The network.** Requests are signed (SigV4); command integrity comes
  from ML-DSA-65; results are encrypted and authenticated under the history
  code, so neither the provider nor a network attacker reads them.
- **A stolen staging command key** runs allowlisted commands only, on
  staging hosts that pin it, until they are wiped: it can stop, start or
  kill a node and read its status and journal, whose results it cannot open
  without the history code. It cannot install, restore or touch keys.
- **A compromised host** is root already. Its keys are write-only for its
  results and history and read-only for commands; it cannot place commands.
  The provider must support prefix-scoped keys.
- **Production.** The rebuild from the provider's image before the
  production install removes everything, wipe removes the agent, and the
  install refuses a host that has it.

## Decided by P01

Approved as proposed on 14 October 2026
([approval](../../../launch/approvals/P01_T06_STAGING_AGENT_2026-10-14.json)):

1. **Key custody:** a dedicated staging command key in its own encrypted
   keystore on the founder's machine, never on a fob or with a kit, retired
   at the final wipe.
2. **Result encryption under the history code,** as the plan says ("like
   the metric history"); no separate result code.
3. **Store layout:** the staging bucket at the storage provider, commands
   under `commands/CHAIN/HOST/`; the founder's own key on the founder's
   machine, **one read-only key per host** limited to that host's command
   prefix, and the existing write-only history key for results.
4. **The allowlist** above, a one-minute poll and a 24-hour validity limit.
