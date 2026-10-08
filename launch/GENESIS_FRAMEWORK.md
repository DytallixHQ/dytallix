# Genesis framework

What Dytallix mainnet's genesis contains, who controls what, how the keys are
made, how the genesis is built and signed, and what is still open. This page
summarizes the approved records it links to; where it and a linked approval
differ, the approval governs. It does not authorize a launch.

## At a glance

| | At genesis |
| --- | --- |
| **Chain** | `dytallix-mainnet-1`, shown as Dytallix ([chain identity](approvals/P01_E05_CHAIN_IDENTITY_2026-10-03.json)) |
| **Cryptography** | ML-DSA-65 for accounts, validators and peers; ML-KEM-768 peer encryption; SLH-DSA-SHAKE-256s root keys. Nothing classical. |
| **DGT** | 1,000,000,000, all issued at genesis, never minted again |
| **DRT** | The fee token, issued over time; a liquid bootstrap at genesis (amount open) |
| **Root controls** | Five key kits held by five people, one each; any three act ([root key holders](approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)) |
| **Network** | One validator, one sentry (the archive) and one endpoint ([solo launch](approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)) |
| **Disclosure** | The [trust model](TRUST_MODEL.md) states every concentration of control |

## 1. Tokens

### DGT: the fixed-supply token

One billion DGT at six decimals (`1,000,000,000,000,000 udgt`), all issued at
genesis. No DGT is minted after genesis and none is burned in the initial
protocol; penalized DGT goes to an escrow nothing can spend (D05-Q02,
[DGT tokenomics](DGT_TOKENOMICS.md)).

| Bucket | Share | DGT | At genesis |
| --- | ---: | ---: | --- |
| Ecosystem growth | 30% | 300,000,000 | The founder's bucket account. The validator's self-bond and the public review's bug bounty come from it. |
| Team and advisors | 20% | 200,000,000 | 199,900,000 in the founder's bucket account; 100,000 to the five root key holders, 20,000 each, unlocked |
| Public sale | 15% | 150,000,000 | The founder's bucket account |
| Private sale | 15% | 150,000,000 | The founder's bucket account |
| Reserve | 20% | 200,000,000 | The founder's bucket account |
| **Total** | **100%** | **1,000,000,000** | |

- A bucket's name does not lock it, and production vesting is neither
  approved nor built. Every movement out of a bucket is a public on-chain
  transfer.
- No DGT is created for validators on top of the total: initial stake moves
  or encumbers existing units ([genesis specification](GENESIS_SPEC.md)).
- Governance votes with bonded DGT only, so liquid holdings carry no vote.

### DRT: the fee and reward token

- **Fees** are paid in DRT, never DGT.
- **Issuance** is adaptive and split each epoch: validators 40%, stakers
  30%, treasury 30%. The epoch is 17,280 blocks, about a day
  ([values](E05_VALUES.json)).
- **Bootstrap** (D08-Q02, approved in principle): ordinary, transferable DRT
  in the genesis supply, assigned to named accounts so the first fees,
  bonds and claims can be paid. The amount and the accounts are open
  (D08-Q03).
- **Proposed, not approved:** include each root key holder's account in the
  bootstrap, since a holder needs a little DRT to move or stake their DGT.

## 2. Accounts at genesis

| Account | Held by | Funded with | Key |
| --- | --- | --- | --- |
| Five bucket accounts | The founder | The bucket amounts above | The founder's own custody, outside the key kits |
| Treasury | The founder | Open (D02-Q02) | The founder's own custody |
| Five holder accounts | One root key holder each | 20,000 DGT each | The holder's wallet fob |
| Validator | The founder | Self-bond from Ecosystem growth | Consensus key in the validator's sealed bundle |

Each account is listed publicly with its purpose. Holder accounts are listed
as "kit holder 1" to "kit holder 5", never by name.

## 3. Root authority

### The kits

Each of the five kits holds four keys, one per role, all derived from the
kit's single 32-byte secret. The node refuses a key that holds two roles.

| Role | What three kits can do |
| --- | --- |
| **Genesis** | Sign the genesis. A production node opens a chain only from a genesis signed by three of five. |
| **Upgrade** | Approve upgrades, release handovers and restarts after a halt |
| **Freeze** | Freeze the chain in an emergency |
| **Resume** | End a freeze |

- **Holders.** Five people, the founder and four others, hold one kit
  each. Any three holders can act; no one, and no two, can. Holders are not
  named in any public record.
- **Limits.** The kits control the chain, not funds: no kit can move a
  token. A control signs a window of at most 34,560 blocks (two days)
  ([control signing](approvals/P01_E05_CONTROL_SIGNING_2026-10-06.json)); a
  release handover takes effect at least 120,960 blocks (seven days) after
  it is admitted; a restart never rewrites history.
- **Signing.** The founder prepares a request online; each holder signs it
  offline with their kit and sends back the signature files, which are
  public; any three are assembled into the control
  ([control signing](../node/docs/mainnet/control-signing.md)).
- **Losing a kit.** Each kit is on one fob with no backup. A lost fob, or a
  holder who leaves, still leaves four kits. **Kit replacement**, a control
  signed by three kits that installs a new kit for a seat under a new
  authority epoch, is approved to be built before launch. Until it exists
  the key sets are fixed in the genesis-bound configuration.

### The key ceremony

Each holder makes their own kit in their own session
([key ceremony](custody/KEY_CEREMONY.md)):

1. A laptop booted from a Debian live USB, network off; its disk is never
   used.
2. The release's `dytallix-root-sign`, checked against the release
   checksums.
3. The kit fob encrypted (LUKS2) under the holder's own passphrase;
   `dytallix-root-sign kit -number N` writes the four private keys to it and
   the public keys to a separate public stick.
4. The fob checked against the public keys, and a proof of possession
   signed for each role (chain, controller, purpose, epoch, session).
5. Only the public stick leaves. No one, the founder included, sees another
   holder's kit secret. There is no paper copy.

Each holder also gets a **separate wallet fob** for their DGT account, so a
kit fob never touches a networked machine. The founder then builds the
genesis signer policy from the five public genesis keys. The whole ceremony
is rehearsed first with throwaway sticks.

A second offline session, the founder's, makes every host's node keys once
the hosts are rented, and seals them into each host's bundle. Its seal
codes and the chain's backup code stay on paper, written twice; since the
kits now have no paper and belong to different people, the founder keeps the
two copies in two separate places of their own
([host setup](../node/docs/architecture/host-setup-v1.md),
[disaster recovery](../node/docs/architecture/disaster-recovery-v1.md)).

## 4. Building and signing the genesis

- **What is signed.** The root bundle binds four files: the native genesis,
  the application configuration (which carries the root authority's key
  sets and bounds), the engine genesis and the release manifest. Each
  genesis signature covers the bundle's SHA-512
  ([root genesis signing](../node/docs/mainnet/root-genesis-signing.md)).
- **Reproducible.** A clean CI runner and a fresh container each rebuild the
  release and the genesis files byte for byte.
- **When.** Holders sign only after the 30-day public review (with a
  separate review by AI, labeled as AI), release acceptance (E06) and gate
  acceptance ([activation](approvals/P01_E05_ACTIVATION_2026-09-30.json)).
- **Genesis time.** Set at the final input freeze: 14:00:00 UTC on a
  weekday, at least 72 hours after the final build. In that window the
  digest is reproduced, three holders sign, and the hosts install and wait.
  If the signatures or the go/no-go six hours before are not complete, a new
  genesis time is set.

## 5. Network at genesis

- **Validator:** private, at one hosting provider, reachable only from its
  sentry.
- **Sentry:** at a different provider from the validator; the archive of
  every block; writes a daily snapshot (with the light blocks a restore
  needs, being built) and pushes it encrypted to object storage at a second
  provider.
- **Endpoint:** serves clients over the post-quantum client channel and a
  public status page.
- **Hosts:** Ubuntu 24.04, each host's public address on its own interface,
  installed from sealed bundles
  ([hosts](approvals/P01_E05_HOSTS_2026-10-06.json)).

More validators join later through on-chain registration.

## 6. Order of work

1. Choose the hosting providers and the storage provider (D12-Q03).
2. Build kit replacement; confirm the four other holders.
3. Rehearse the ceremony; each holder makes their kit and wallet fob.
4. Fill and check the emergency, upgrade and genesis custody packets.
5. Rent the hosts; the node key session; the staging runs on the
   production hosts (E03, the T suites, the seven simulations) with
   rehearsal identities; wipe.
6. Freeze the release candidate (E06); the 30-day public review; fixes.
7. Final input freeze: the allocation ledger (bucket, holder, validator and
   DRT bootstrap rows) and the genesis time.
8. Reproduce, sign (three of five), install, start.

## 7. Open decisions

| Item | Register | State |
| --- | --- | --- |
| Kit replacement control design | D10-Q03, D11-Q03 | Approved to build; design to P01 |
| Custody packet model for five holders (an outside reviewer, or public disclosure) | D10-Q03 | Open |
| DRT bootstrap amount and accounts, holders included | D08-Q03 | Open |
| Recipient rows, wallet addresses and any vesting | D08-Q01 | Holder grants approved; addresses open |
| Treasury reward recipient and custody | D02-Q02 | Open |
| Validator self-bond amount | D09-Q02 | Open |
| Where the founder keeps the bucket and treasury keys | — | Open |
| Hosting and storage providers | D12-Q03 | Open |

## 8. What is public and what is not

- **Public:** the genesis files and their digests, the five kits' public
  keys and proofs, every genesis account and its purpose, the pin plan, the
  release and its checksums, and this framework.
- **Never public:** any private key or kit secret, passphrases, seal and
  backup codes, the holders' names, and where kits and fobs are kept.
