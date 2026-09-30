# Whitepapers

The three core Dytallix whitepapers are bundled directly in this repository so
they can be referenced from the docs site and the GitHub repo without relying on
external file hosting.

Use the whitepapers for long-form design context. Several of their claims are
superseded by the mainnet candidate; see
[Errata for the mainnet candidate](#errata-for-the-mainnet-candidate).

## Recommended Reading Order

1. foundational
2. technical
3. tokenomics

## Bundled PDFs

### Foundational White Paper

- file: [Dytallix Foundational White Paper](assets/whitepapers/dytallix-foundational-white-paper.pdf)
- pages: `28`
- focus: product vision, protocol framing, and the long-form conceptual model

### Technical White Paper

- file: [Dytallix Technical White Paper](assets/whitepapers/dytallix-technical-white-paper.pdf)
- pages: `15`
- focus: technical architecture, developer-facing mechanics, and implementation
  context

### Tokenomics Paper

- file: [Dytallix Tokenomics Paper](assets/whitepapers/dytallix-tokenomics-paper.pdf)
- pages: `20`
- focus: dual-token economics, incentive design, and governance-related economic
  structure

## Important Caveat

The whitepapers describe an earlier protocol design. The PDFs cannot be
edited. The mainnet candidate differs from them in consensus, penalties,
fees, governance, oracles and interoperability. The errata below list each
difference.

The public testnet is a separate chain and differs from both. For example,
its node reported `udgt` as the fee denomination in April 2026, while the
mainnet candidate charges fees in uDRT.

For builders, use the whitepapers for context only. Use the errata and the
linked architecture documents for the mainnet candidate's behavior.

## Errata for the mainnet candidate

Each entry names a claim in a paper, what the mainnet candidate does instead,
and where that is defined. Section numbers refer to the PDFs. Decision IDs
(such as D07-Q01) refer to `mainnet/launch/MAINNET_DECISION_REGISTER.json`.
Row and conflict IDs (such as CONS-001 and AC-004) refer to the
[E04 requirement triage](../../node/docs/mainnet/e04-requirement-triage.md).
Where a value is still an open decision, the entry says so.

### Tokenomics Paper

**Vote decay, vote delegation and VRF sortition** (§2.1, §9.1.4, §10.2)

- Paper: DGT voting power decays when a staked position does not vote, and a
  vote or a Refresh transaction resets it. DGT is delegated to validators to
  raise their chance of leader election by VRF sortition.
- Mainnet candidate: governance weight is linear in stake. Each account
  votes with its own effective bonded DGT at the proposal's snapshot.
  Liquid, queued and non-effective DGT count zero. There is no decay, no
  Refresh transaction and no delegation of votes. Proposers follow the
  CometBFT state machine; there is no VRF sortition.
- Defined in: [governance v1](../../node/docs/architecture/governance-v1.md)
  (electorate and tally); triage AC-006, GOV-004 and CONS-001.

**Fee formula and fee split** (§4.1, §10.7)

- Paper: a fee is a compute charge plus a utilization-priced bandwidth charge
  plus a priority tip. The bandwidth charge is burned; the block proposer
  receives the compute charge and the tip.
- Mainnet candidate: fees are paid in uDRT, with no tips. The charge is
  `max(measured gas, minimum gas) × gas price`, one price on a combined gas
  total. Failed and out-of-gas transactions pay too. Every transaction fee
  (ordinary, governance and recovery) is burned, and so is the account
  creation fee. Validators are paid from issuance only.
- Defined in: [fees v1](../../node/docs/architecture/fees-v1.md); triage
  AC-002 and AC-013.

**Fee floor for validator viability** (§4.3)

- Paper: the emission controller enforces a fee floor derived from the DRT
  price and real bandwidth cost, so validators stay solvent.
- Mainnet candidate: there is no fee floor tied to prices or validator
  costs. The gas price is a fee profile value, set at genesis and changed
  only by governance within genesis bounds; the minimum gas changes only by
  upgrade. The minimum fee protects against spam, not validator income.
  Validators receive the 40% validator share of issuance, divided every
  block by voting power among the active validators.
- Defined in: [fees v1](../../node/docs/architecture/fees-v1.md) (option A
  and validator rewards);
  [governance v1](../../node/docs/architecture/governance-v1.md) (governed
  parameters); triage AC-002.

**Oracle Medianizer** (§3.1, §4.3.1, §8.4, §10.5, §10.6)

- Paper: validators report prices and a volatility index; an on-chain
  medianizer aggregates them, and outlier reporters are slashed.
- Mainnet candidate: the consensus build has no oracle and no reporters, so
  there is no medianizer, no outlier detection and no oracle slashing. The
  only issuance input is the epoch observation, which every validator
  derives from committed blocks: utilization is the epoch's transaction
  bytes over its block capacity, and volatility is 0. CheckTx refuses a
  submitted observation. This observation contract is approved, and no
  external oracle is used at launch (D01-Q02, D07-Q01, P01, 29 September 2026).
- Defined in:
  [adaptive emission v1](../../node/docs/mainnet/adaptive-emission-v1.md)
  (observation contract v1); triage ORC-001 to ORC-003, AC-001 and AC-010.

**Liquidity bootstrapping pool and USDC floor** (§2.3)

- Paper: a 95/5 DRT and USDC liquidity bootstrapping pool sets a DRT floor
  price before the first epoch.
- Mainnet candidate: there is no liquidity bootstrapping pool, no USDC or
  other wrapped asset and no floor price; the pool and wrapped USDC are POST
  MAINNET (D07-Q01, P01, 29 September 2026). The first users get DRT for
  fees from a liquid genesis bootstrap assigned to named accounts (D08-Q02);
  its amount and recipients are genesis inputs (D08-Q02, D08-Q03).
- Defined in: triage BRG-001 and AC-009.

**MPC Airlock** (§7.3)

- Paper: legacy assets migrate through multi-party computation custody and a
  proof of reserve, and PQC-native tokens are minted against them.
- Mainnet candidate: the consensus build has no Airlock, no bridge and no
  multi-party custody. The Airlock and bridges are POST MAINNET (D07-Q01,
  P01, 29 September 2026).
- Defined in: triage BRG-001, BRG-002 and AC-009; the bridge boundary in
  the [security model](security-model.md#bridge-boundary).

### Technical White Paper

**Checkpoint finality gadget** (§6, §6.1 to §6.3)

- Paper: a Nakamoto-style chain of 2-second slots with VRF leader election
  and LMD-GHOST fork choice, finalized by BFT checkpoint blocks signed at the
  end of each epoch.
- Mainnet candidate: consensus is a fork of CometBFT v0.40.0, with ML-DSA-65
  validator keys and votes. It runs the upstream CometBFT state machine with
  a quorum of more than two thirds of voting power; a block is final when it
  is committed. There are no slots, no fork choice and no checkpoint blocks.
  A node that joins by state sync verifies signed light blocks with
  CometBFT's light client. Block time, timeouts and fault assumptions are
  open (D06-Q02).
- Defined in: triage CONS-001 and AC-004;
  [state sync v1](../../node/docs/architecture/state-sync-v1.md) (light
  blocks); the engine in `node/consensus/cometbft`.

**"Double signing burns 100% of stake"** (§6.4; repeated in the tokenomics
paper, §7.1 and §9.1.3)

- Paper: equivocation forfeits the whole stake, so finalizing a conflicting
  chain costs at least one third of the stake.
- Mainnet candidate: no stake is burned for double signing. Until the
  penalty rules are decided, duplicate-vote and light-client-attack evidence
  is recorded only, with no effect on stake or the validator set. Production
  penalties are refused, and there is no jailing. Which faults are penalized,
  and at what rates, is open (D09-Q04, D09-Q05). Two quorums of more than two
  thirds still overlap in more than one third of voting power, but that
  bound is not a monetary loss.
- Defined in: [liveness v1](../../node/docs/architecture/liveness-v1.md)
  (evidence); triage CONS-002, VAL-002 and AC-003;
  [specification decisions](../../node/docs/mainnet/specification-decisions.md)
  (mathematical corrections).

**Contract execution** (§8, the Dytallix Virtual Machine)

- Paper: a WASM virtual machine with precompiled contracts.
- Mainnet candidate: the consensus build has no contract runtime. It runs
  ordinary, governance and recovery transactions only. A contract runtime
  is POST MAINNET (D07-Q01, P01, 29 September 2026).
- Defined in: the SDK's
  [core concepts](../../sdk/docs/core-concepts.md); the
  [contract quickstart](contract-quickstart.md).

### Foundational White Paper

**Checkpoints and LMD-GHOST** (§5, §9, §14.1, §14.2, §14.6.2)

- Paper: Nakamoto-style proof of stake with VRF leader election, LMD-GHOST
  fork choice and BFT epoch checkpoints; light clients accept a header chain
  anchored by a finalized checkpoint. Equivocation costs 100% of stake and
  liveness faults 0.1% (§9.5, §14.4).
- Mainnet candidate: CometBFT consensus with commit finality, as in the
  technical paper entry above. There is no VRF, fork choice, checkpoint
  block or checkpoint attestation. Evidence is recorded only until the
  penalty rules are decided (D09-Q04, D09-Q05).
- Defined in: triage CONS-001 and AC-004;
  [liveness v1](../../node/docs/architecture/liveness-v1.md);
  [state sync v1](../../node/docs/architecture/state-sync-v1.md).

**Governance-mutable parameters and the algorithm registry** (§12.3, §14.5)

- Paper: governance can change fee-market coefficients, emission controller
  parameters and gains, treasury spending, protocol timing, and the
  lifecycle state of cryptographic algorithms through an on-chain registry.
- Mainnet candidate: governance has two action classes. A parameter change
  sets one of three governed parameters, within genesis bounds: the
  ordinary fee profile (gas price, per-resource costs and the account
  creation fee, as a new profile version), `min_self_bond` or `max_active`.
  A validator registry change adds or removes a validator operator. Emission,
  governance's own rules, penalty rates, evidence limits, DGT supply,
  signature and algorithm profiles and the recovery template change only by
  upgrade, and upgrades are root-signed only. There is no on-chain algorithm
  registry: the algorithm set changes only by a root-signed upgrade.
  Treasury spending is POST MAINNET. Governance's numeric values (quorum,
  thresholds, deposit, periods, timelock) are open (D11-Q02).
- Defined in: [governance v1](../../node/docs/architecture/governance-v1.md)
  (rule 3 and decisions); [upgrade
  execution](../../node/docs/mainnet/upgrade-execution.md); triage AC-007.

**The Airlock** (§4.5, §15, §18.1, glossary)

- Paper: legacy-chain value enters through an Airlock migration process
  instead of a bridge.
- Mainnet candidate: there is no Airlock or migration interface, and no
  bridge. The Airlock is POST MAINNET (D07-Q01, P01, 29 September 2026).
- Defined in: triage BRG-001 and AC-009; the bridge boundary in the
  [security model](security-model.md#bridge-boundary).

**WASM runtime and PQC precompiles** (§5, §10.1, §10.3)

- Paper: an execution layer with a WASM runtime and PQC host precompiles.
- Mainnet candidate: no contract runtime, as in the technical paper entry
  above (D07-Q01).
- Defined in: the SDK's [core concepts](../../sdk/docs/core-concepts.md).
