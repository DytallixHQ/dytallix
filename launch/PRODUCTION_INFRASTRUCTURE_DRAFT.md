> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Production infrastructure draft

Status: **DRAFT. NOT APPROVED. NOT IMPLEMENTED.**

This draft defines a candidate architecture and its required records. It does not select a provider, buy resources, assign operators, or approve launch. The approved mainnet inventory is empty. Batch 1 verified one existing Hetzner host and six service observations. No provider resources, addresses, operator identities, budgets, or test results were invented.

The companion [JSON record](/Users/rickglenn/Developer/Dytallix-mainnet-launch/PRODUCTION_INFRASTRUCTURE_DRAFT.json) contains the asset schema, empty inventories, record templates, decisions, and acceptance checks. The [static assessment](/Users/rickglenn/Developer/Dytallix-mainnet-launch/assessment/release-operations.md) identifies current release and operations blockers. Frozen source snapshots remain unchanged.

## Candidate architecture

| Component | Proposed boundary | Decision still required |
|---|---|---|
| Validators | Private validator hosts. Expose peer traffic only through approved sentries. Keep public RPC and unrelated services off validator hosts. | Consensus protocol, validator count, voting weights, operators, placement, capacity, and access policy. |
| Sentries | Sentries relay approved peer traffic. Propose at least two reachable paths per validator across approved failure domains. Sentries hold no validator signing keys. | Exact count, peer policy, placement, ingress limits, and path independence. |
| Seed and peer discovery | Maintain approved bootstrap and persistent-peer records. Verify peer identity and network binding. | Discovery mechanism, seed count, ownership, recovery, and distribution process. |
| RPC services | Propose at least two independent RPC backends behind redundant ingress. Use verified state and release compatibility in health decisions. | Hardware, placement, interfaces, quotas, freshness policy, load limits, DNS, and certificates. |
| Monitoring | Collect consensus, validator, transaction, economic, host, and RPC metrics. Restrict collection endpoints. Monitor collector and alert-delivery failure independently. | Collector placement, retention, thresholds, availability targets, and access. |
| Logs | Collect structured, redacted logs. Rotate local logs. Control central access and retention. | Retention periods, storage capacity, redaction checks, and incident access. |
| Backups | Create consistent state snapshots. Store encrypted copies outside the source failure domain. Keep key backups under separate custody controls. | Snapshot method, trust checks, destinations, retention, schedule, and recovery objectives. |
| Release services | Build from frozen inputs. Distribute checksummed binaries and the required container image. Verify the release manifest before installation. | Build environment, registry, provenance verification, access, retention, and approval process. |
| Administration | Use a restricted management path with named operators and recorded access approval. | Access mechanism, emergency access, account separation, and session records. |

These components are proposals. The existing host is recorded separately in [Batch 1 operations evidence](batch-1/OPERATIONS_EVIDENCE.md). The node uses PM2 as root. Provider-wide inventory, named operators, custody, backup acceptance, monitoring acceptance, and source-to-binary provenance remain open.

## Validator count and failure domains

The consensus protocol remains a required decision. Do not infer a validator count from the current single-validator testnet.

Four equal-weight validators are a conditional minimum only if the selected Byzantine fault tolerant protocol uses more than two-thirds voting power for finality and tolerates one Byzantine validator. This count does not qualify the implementation or establish decentralization.

For tolerance of an entire domain outage, each tolerated independent failure domain must contain less than one-third of total voting power. The remaining voting power must exceed two-thirds. Count shared operator control, provider, region, account, network, and signing dependencies. Four hosts under one administrator do not establish four independent control groups.

Record weighted exposure to each shared dependency. Validate the selected failure combinations against the approved consensus assumptions. State founding-team control explicitly if it remains substantial.

## Signing and custody boundaries

Validator signing keys must remain outside sentries, RPC servers, build runners, and monitoring services. Select a custody backend only after verifying support for the approved post-quantum algorithm and node signing interface. Do not assume that an available hardware security module supports the selected algorithm.

Each validator needs a custody record. Record its public-key fingerprint, algorithm specification, operator, generation ceremony, access policy, backup policy, and recovery evidence. Reference secrets through controlled operator systems. Store no private key, recovery seed, token, or passphrase in this draft.

The custody procedure must persist required signing state. Fence the previous signer before replacement or restoration. Fencing prevents the previous signer from remaining active. A key backup alone does not establish safe validator recovery.

Define rotation, compromise response, emergency replacement, and authorization. Separate validator signing authority from governance and treasury authority. Production governance remains mandatory for initial launch.

## Monitoring and alert coverage

Required signals include block and finalized height, consensus rounds, validator participation, missed blocks, voting power, peers, mempool size, throughput, transaction failures, gas, fees, rewards, DRT issuance, DGT supply, burns, delegation, slashing, governance, CPU, memory, disk growth, network use, RPC latency, RPC errors, signing latency, verification latency, restarts, and availability.

Required alerts include consensus halt, divergence, validator outage, abnormal missed blocks, disk exhaustion, RPC outage, supply invariant failure, unexpected mint or burn, abnormal staking or governance events, repeated signature verification failures, restart loops, stale backups, failed collection, and failed alert delivery.

For each alert, record the signal, threshold, duration, severity, owner, delivery route, acknowledgement limit, escalation rule, recovery condition, and runbook. Thresholds remain unset until the approved protocol and measured capacity support them.

The static assessment found missing validator and oracle alert inputs. Draft records do not repair those inputs. Link required oracle monitoring to the approved economic model.

## Recovery and upgrades

Recovery time objective (RTO) is the maximum approved service recovery time. Recovery point objective (RPO) is the maximum approved age of recoverable data. Both values remain `null` for validators, RPC, monitoring, and backup restoration.

A backup interval does not authorize loss of committed chain state. Recovery must reconstruct verified authoritative state. Validate chain identity, genesis, committed state, transaction receipts, staking, rewards, governance, and both token supplies. Validate signing continuity separately.

Specify consistent snapshot creation, snapshot trust, encryption, off-host storage, retention, restoration, and evidence collection. Record the approved source of state when a snapshot is behind the chain. Verify that restoration cannot reactivate a second signer.

Upgrade records must specify governance authority, activation rules, permitted versions, state migration, late validators, old RPC behavior, and rollback limits. Never use a fresh genesis as a mainnet upgrade or recovery procedure. One canonical production genesis remains the requirement.

## Release and configuration records

Every installed asset must reference an approved release manifest. Record the source revision, release tag, compiler, build-environment digest, dependency-lock digests, binary digest, container-image digest, software bill of materials, dependency report, and provenance.

Record genesis, configuration, runbook, and monitoring-configuration digests. Record the actual running artifact separately from the intended artifact. A successful build does not prove installation. A running process does not prove that it uses approved configuration.

The JSON asset schema requires explicit fields for resource identity, environment, role, owner, control group, failure domains, placement, access references, capacity, budget approval, artifact digests, monitoring, backup, custody, and verification evidence. Empty fields cannot qualify an asset as verified.

| Record | Required content |
|---|---|
| Asset | Actual resource reference, placement, role, owner, control group, failure domains, configuration, release, and acceptance evidence. |
| Operator | Assigned person and organization references, responsibilities, assets, primary and backup on-call contacts, access approval, and acceptance. |
| Key custody | Public identity, algorithm, verified backend, access, generation, backup, signing-state policy, fencing, rotation, and recovery evidence. |
| Backup policy | Covered assets, snapshot consistency, trust rule, encryption, destination, retention, schedule, RTO, RPO, and restore evidence. |
| Release | Frozen source, build inputs, required outputs, digests, provenance, genesis, configuration, and approval. |
| Acceptance result | Check, environment, run, candidate digest, assets, operator, independent reviewer, time, result, evidence, and defects. |
| Rehearsal difference | Field, production and rehearsal references, reason, effect on validity, reviewer, and approval. |

## Isolated rehearsal infrastructure

Create a separate environment only after resource and budget approval. Match the intended production topology, validator weighting, software, economic rules, deployment process, upgrade process, monitoring, and runbooks.

Use separate rehearsal identities, signing keys, funds, and routing. Do not copy production private keys or connect rehearsal validators to production peers. Record every permitted chain-identity and genesis substitution. A substitution must preserve the behavior under test.

Approve a distinct rehearsal chain identifier for each clean run. Keep the production genesis process fixed. Rehearsal genesis files do not create additional canonical production networks.

Complete seven runs: clean genesis; infrastructure failure; network fault scenarios; production upgrade; operator error; production load and economic activity; final dress rehearsal. Keep each run within the isolated, approved environment. Record exact manifests, telemetry, state checks, defects, and reviewer decisions. No runs have occurred under this draft.

## Missing decisions and responsibilities

| ID | Decision | Accountable role, unassigned |
|---|---|---|
| INF-01 | Consensus threshold, fault model, timing, and weighting. | Protocol lead. |
| INF-02 | Validator counts, operators, weights, control groups, and domain limits. | Protocol lead and validator coordinator. |
| INF-03 | Providers, regions, zones, account boundaries, and shared dependencies. | SRE lead. |
| INF-04 | Hardware, storage growth, bandwidth, quotas, and budget. | SRE lead and budget owner. |
| INF-05 | Sentries, seeds, peers, management path, interfaces, and firewall policy. | Network engineer. |
| INF-06 | Custody backend, key lifecycle, signing state, and compromise response. | Cryptography lead and validator operators. |
| INF-07 | RTO, RPO, state trust, restoration, and signer fencing. | SRE lead and protocol lead. |
| INF-08 | Availability targets, alerts, retention, on-call, and escalation. | SRE lead. |
| INF-09 | Build inputs, registry, manifests, configuration control, and provenance. | Release engineer. |
| INF-10 | Governed upgrade authority, activation, compatibility, and recovery. | Protocol lead and governance lead. |
| INF-11 | Rehearsal layout, schedule, budget, access, and approved differences. | QA lead and SRE lead. |
| INF-12 | Operators, independent reviewers, incident command, and launch authority. | Release lead. |

The gRPC interface remains a specification decision. This draft does not defer it. Governance, staking, wallets, DRT, DGT, and all required initial mainnet functions remain launch requirements.

## Acceptance checks

All checks have status **NOT RUN** in the JSON record.

1. Verify approved topology, control groups, voting weights, failure domains, and shared dependencies.
2. Verify reproducible binaries, container image, release manifest, checksums, dependencies, and provenance.
3. Verify chain identity, genesis, configuration, peers, clocks, and network restrictions.
4. Verify custody, encrypted key backups, signing state, recovery authorization, and signer fencing.
5. Verify validator participation, state agreement, synchronization, restart, replacement, and recovery.
6. Verify approved RPC interfaces, redundant service behavior, transaction lifecycle, and wallet reconnect.
7. Verify metric collection, alert delivery, acknowledgement, escalation, and monitoring failure detection.
8. Restore services and reconcile state, supply, staking, rewards, and governance within approved recovery objectives.
9. Complete the governed upgrade and permitted recovery procedures with state continuity.
10. Verify production parity and approve every rehearsal difference.
11. Complete all seven required launch runs with candidate-linked evidence.
12. Resolve critical blockers and obtain explicit final launch approval.

No infrastructure readiness percentage or launch date is established by these draft records.
