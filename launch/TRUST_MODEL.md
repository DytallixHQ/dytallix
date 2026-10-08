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
| **Validators** | One validator, run by the founder, behind one sentry, with one endpoint serving clients. | Every block is still checked by post-quantum signatures, and the validator host takes no inbound connections except from its sentry. There is no fault tolerance and no decentralization: if the validator fails, the chain pauses until it is restored, and the founder alone decides what enters blocks. |
| **Root controls** | Five people, the founder and four others, each hold one of the five key kits that carry the genesis, upgrade and emergency keys. Any three kits are needed to act. | No one person, and no two, can sign the genesis, upgrade or freeze the chain. Losing two kits, or having them stolen, gives no one control. It does not protect against three holders acting together or being coerced together. The holders are not named publicly. |
| **Tokens** | The founder holds 99.99% of DGT at genesis, in one public account per bucket (Ecosystem growth 30%, Team and advisors 20%, Public sale 15%, Private sale 15%, Reserve 20%) plus a treasury account. The other 100,000 DGT (0.01%), from Team and advisors, goes to the five root key holders, 20,000 each. | Every movement out of a bucket is a visible on-chain transfer. Nothing stops the founder from moving them: the key holders control the chain, not the buckets. |
| **Review** | The frozen release gets a 30-day public review with a bug bounty paid in DGT, plus a separate AI review labeled as AI. | No independent human audit has been done. The launch is labeled **unaudited** until one is funded. |
| **Operations** | One person, best effort, alerted 24/7 by a free external monitor. Goals: a block in 99.5% of minutes and a working endpoint in 99.0% of minutes each month. | These are goals, not commitments. Expect pauses, especially overnight in the founder's time zone. |
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
  registration, up to 16 active validators under the current genesis bound.
- **Root controls.** A lost kit is replaced, or a seat moves to a new
  holder, through a kit replacement control signed by three kits (P01,
  7 October 2026; built before launch).
- **Tokens.** Sales, grants and team allocations leave their bucket accounts
  as public transfers.
- **Audit.** An independent human audit is commissioned when funding allows,
  and the **unaudited** label is removed only after it.

Each of these steps is published when it happens.
