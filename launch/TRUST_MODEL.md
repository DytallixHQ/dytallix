# Dytallix mainnet launch trust model

Dytallix mainnet launches with one person, the founder, running the network,
and five people holding its root controls. This page states what that means
for anyone who uses the chain or holds its tokens. It is the solo launch
profile approved on 3 October 2026
([approval](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)), with the root
key holders changed on 7 October 2026
([approval](approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)).

## What you are trusting at launch

| Area | At launch | What it protects against, and what it doesn't |
| --- | --- | --- |
| **Validators** | One validator, run by the founder, behind one sentry, with one endpoint serving clients. The endpoint may run as its own virtual machine on the founder's web server; the validator and the sentry have hosts of their own. | Every block is still checked by post-quantum signatures, and the validator host takes no inbound connections except from its sentry. There is no fault tolerance and no decentralization: if the validator fails, the chain pauses until it is restored, and the founder alone decides what enters blocks. |
| **Root controls** | Five people, the founder and four others, each hold one of the five key kits that carry the genesis, upgrade and emergency keys. Any three kits are needed to act. | No one person, and no two, can sign the genesis, upgrade or freeze the chain. Losing two kits, or having them stolen, gives no one control. It does not protect against three holders acting together or being coerced together. The holders are not named publicly. |
| **Tokens** | The founder holds 99.99% of DGT at genesis, in one public account per bucket (Ecosystem growth 30%, Team and advisors 20%, Public sale 15%, Private sale 15%, Reserve 20%) plus a treasury account. The other 100,000 DGT (0.01%), from Team and advisors, goes to the five root key holders, 20,000 each. | Every movement out of a bucket is a visible on-chain transfer. Nothing stops the founder from moving them: the key holders control the chain, not the buckets. |
| **Review** | Hashlock, an independent security firm, audits the frozen release candidate, while a 30-day public review with a bug bounty paid in DGT and a separate AI review labeled as AI run alongside ([audit](approvals/P01_E06_AUDIT_2026-10-10.json)). | Every finding is fixed or answered on the record. The launch is labeled **unaudited** until Hashlock's report is published. An audit lowers the risk of defects; it does not prove there are none. |
| **Operations** | The founder, best effort, alerted 24/7: a monitor job on each host pushes heartbeats and alerts to a free alerting service, and a free uptime checker watches the public status page ([monitoring](approvals/P01_E05_MONITORING_2026-10-09.json)). A severity 1 alert not acknowledged within 15 minutes also goes to one backup person, a root key holder. Goals: a block in 99.5% of minutes and a working endpoint in 99.0% of minutes each month. | These are goals, not commitments. Expect pauses, especially overnight in the founder's time zone. |
| **History and backups** | Full history on one archive node plus a monthly offline export. Daily encrypted snapshots on a second provider. | A restore never rolls back committed blocks. A single archive operator means a single party holds the full history. |

## What is the same as a larger launch

- **Cryptography.** The same post-quantum protocol: ML-DSA-65 signatures,
  ML-KEM-768 peer encryption and SLH-DSA root controls, with no classical
  fallback.
- **Reproducible builds.** Anyone can rebuild the release and the genesis
  from source and check them byte for byte. A clean CI runner and a fresh
  container do so before launch.
- **Published configuration.** The genesis, the network identity
  (`dytallix-mainnet-1`) and the pin plan's digests are published.

## How this changes over time

- **Validators.** Other operators can join through on-chain validator
  registration, up to 16 active validators, a limit governance can raise to
  32 within the genesis bounds.
- **Root controls.** A lost kit is replaced, or a seat moves to a new
  holder, through a kit replacement control signed by three kits' upgrade
  keys, public on chain for seven days before it takes effect (P01, 7 and
  8 October 2026; built).
- **Tokens.** Sales, grants and team allocations leave their bucket accounts
  as public transfers.
- **Audit.** The **unaudited** label is removed when Hashlock's report is
  published, with every finding fixed or answered on the record.

Each of these steps is published when it happens.
