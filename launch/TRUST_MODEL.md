# Dytallix mainnet launch trust model

Dytallix mainnet launches with one person, the founder, running the network
and holding its controls. This page states what that means for anyone who
uses the chain or holds its tokens. It is the solo launch profile approved on
3 October 2026 ([approval](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)).

## What you are trusting at launch

| Area | At launch | What it protects against, and what it doesn't |
| --- | --- | --- |
| **Validators** | One validator, run by the founder, behind one sentry, with one endpoint serving clients. | Every block is still checked by post-quantum signatures, and the validator host takes no inbound connections except from its sentry. There is no fault tolerance and no decentralization: if the validator fails, the chain pauses until it is restored, and the founder alone decides what enters blocks. |
| **Root controls** | The founder holds every genesis, upgrade and emergency key in five key kits stored in separate places. Any three kits are needed to act. | Losing or having two kits stolen does not give anyone control. It does not protect against the founder, or against someone coercing the founder. |
| **Tokens** | The founder holds 100% of DGT at genesis, in one public account per bucket (Ecosystem growth 30%, Team and advisors 20%, Public sale 15%, Private sale 15%, Reserve 20%) plus a treasury account. | Every movement out of a bucket is a visible on-chain transfer. Nothing stops the founder from moving them. |
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
- **Root controls.** Key seats move to other holders through a signed
  upgrade, until no one person can reach three.
- **Tokens.** Sales, grants and team allocations leave their bucket accounts
  as public transfers.
- **Audit.** An independent human audit is commissioned when funding allows,
  and the **unaudited** label is removed only after it.

Each of these steps is published when it happens.
