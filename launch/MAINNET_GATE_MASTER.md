# Dytallix mainnet master gate list

Canonical data: [LAUNCH_GATES.json](LAUNCH_GATES.json). This document is generated from that file. Edit the JSON, then regenerate this view. Separate packages are evidence archives.

Launch: **NO GO**. Formal acceptance: **0/35**. Production records accepted: **0/8**. Final simulations completed: **0/7**.

Current counts: 0 OPEN, 35 PARTIAL, 0 READY FOR ACCEPTANCE, 0 PASS. Partial progress can consist of preparation, implementation or local tests. It is not a percentage of completed acceptance criteria.

The gate IDs retain their original repository meanings. The [conversation crosswalk](decision-register/gate-consolidation-20260912/CONVERSATION_CROSSWALK.json) maps the proposed work list without renumbering gates.

Use the [reconciliation report](decision-register/gate-consolidation-20260912/RECONCILIATION_REPORT.md) for source drift, superseded claims and validation limits. The [evidence index](decision-register/gate-consolidation-20260912/PRIMARY_EVIDENCE_INDEX.json) preserves all indexed supporting records. Do not add overlapping test counts.

| Gate | Requirement category | Status |
|---|---|---|
| [G01](#g01) | Consensus stability | PARTIAL |
| [G02](#g02) | Consensus safety | PARTIAL |
| [G03](#g03) | PQC correctness | PARTIAL |
| [G04](#g04) | Genesis reproducibility | PARTIAL |
| [G05](#g05) | Genesis allocation correctness | PARTIAL |
| [G06](#g06) | Binary reproducibility | PARTIAL |
| [G07](#g07) | Validator deployment | PARTIAL |
| [G08](#g08) | Validator recovery | PARTIAL |
| [G09](#g09) | State recovery | PARTIAL |
| [G10](#g10) | PQC transaction lifecycle | PARTIAL |
| [G11](#g11) | Wallet reliability | PARTIAL |
| [G12](#g12) | DRT transfers | PARTIAL |
| [G13](#g13) | DGT transfers | PARTIAL |
| [G14](#g14) | DRT supply accounting | PARTIAL |
| [G15](#g15) | DGT supply accounting | PARTIAL |
| [G16](#g16) | Staking | PARTIAL |
| [G17](#g17) | Delegation | PARTIAL |
| [G18](#g18) | Undelegation | PARTIAL |
| [G19](#g19) | Rewards | PARTIAL |
| [G20](#g20) | Fees | PARTIAL |
| [G21](#g21) | Slashing | PARTIAL |
| [G22](#g22) | Governance | PARTIAL |
| [G23](#g23) | Treasury controls | PARTIAL |
| [G24](#g24) | Upgrade procedure | PARTIAL |
| [G25](#g25) | RPC stability | PARTIAL |
| [G26](#g26) | Load capacity | PARTIAL |
| [G27](#g27) | Monitoring | PARTIAL |
| [G28](#g28) | Alerting | PARTIAL |
| [G29](#g29) | Backup procedures | PARTIAL |
| [G30](#g30) | Disaster recovery | PARTIAL |
| [G31](#g31) | Security | PARTIAL |
| [G32](#g32) | Documentation | PARTIAL |
| [G33](#g33) | Developer onboarding | PARTIAL |
| [G34](#g34) | Validator onboarding | PARTIAL |
| [G35](#g35) | No quantum-vulnerable asymmetric cryptography inside the production trust boundary | PARTIAL |

## Status and acceptance rules

- **OPEN:** No credited implementation or preparation evidence for the requirement.
- **PARTIAL:** Credited implementation, tests or preparation exist; required implementation, qualification or acceptance is incomplete.
- **READY FOR ACCEPTANCE:** All implementation and candidate qualification evidence is complete; only authorized formal acceptance remains.
- **PASS:** Every formal acceptance requirement is satisfied with current candidate-bound evidence and named authorized independent acceptance.

All 35 gates and eight production records must be accepted for the final compatible candidate set. Complete seven valid simulations under MAINNET_EXECUTION_PLAN.md and obtain explicit launch authorization. No gate status alone authorizes deployment.

<a id="g01"></a>
## G01 — Consensus stability

**Status: PARTIAL**

**Requirement:** Freeze the production consensus profile and limits. Demonstrate stable block production, validator participation and recovery under the approved fault and load conditions.

**Completed work**

- Batch 7 connected CometBFT, ABCI and atomic Rust application commits. Four validators passed quorum-loss and restart checks.
- Batch 8 checked six-node membership and H+2 power transitions.
- The earlier TCP RPC engine candidate passed 154 assertions and six transport checks across four local validators.
- The new private Unix RPC engine and Hyper HTTP adapter passed 154 actual consensus/RPC assertions and six transport checks across four engines. Submission, commitment, receipts and restart used the retained application to isolate the interface change.
- The Linux ARM64 application built from 365 unchanged Rust/Cargo inputs. Four actual restricted systemd services passed 36 checks, including commitment, quorum loss/restoration and restart. This uses the retained TCP RPC engine, not the new IPC/Hyper service profile.
- The selected root-enabled stack passed 182 consensus/client checks plus 6 transport checks natively and 37 checks through actual Linux x86_64 validators. Artifact and source identities remain separately recorded for each run.
- Four native replicas committed ordinary transactions, freeze and resume. They agreed on receipts and control state. Blocks continued during freeze.
- Integrated version 2 emergency anchors and atomic upgrade controls in consensus settlement. Local tests cover mandatory transitions, same-block freeze priority, replay and restart.

**Evidence**

- [batch-7/APPROVAL.json](batch-7/APPROVAL.json)
- [batch-7/REPORT.md](batch-7/REPORT.md)
- [batch-7/TEST_RESULTS.json](batch-7/TEST_RESULTS.json)
- [batch-7/VALIDATION.json](batch-7/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/production-integration/pqc/ipc-runtime/RESULTS.json](decision-register/production-integration/pqc/ipc-runtime/RESULTS.json)
- [decision-register/production-integration/pqc/ipc-runtime/REPORT.md](decision-register/production-integration/pqc/ipc-runtime/REPORT.md)
- [decision-register/production-integration/evidence/FINAL_SOURCE_CHECK.json](decision-register/production-integration/evidence/FINAL_SOURCE_CHECK.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/REPORT.md](decision-register/production-integration/linux/retry/REPORT.md)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-boundary-qualification/linux-x86/REPORT.md](decision-register/production-boundary-qualification/linux-x86/REPORT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/consensus/RESULTS.json](decision-register/emergency-upgrade-execution/consensus/RESULTS.json)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve the exact engine version, timing, capacity, fault assumptions, synchronization and retention rules.
- Run the complete frozen Linux application, bridge and engine on the approved topology.
- Measure sustained operation and recovery against approved thresholds; complete the applicable final simulations.

**Acceptance requirement:** Freeze the production consensus profile and limits. Demonstrate stable block production, validator participation and recovery under the approved fault and load conditions.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L08: Controlled staging measurements; approved limits and recovery objectives; no conversion of component rates into TPS.
- L09: Seven complete reviewed frozen-candidate simulation records under the existing execution plan.

**Acceptance authority:** Consensus lead and independent protocol reviewers. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Changed Rust code has local executable evidence. Current Linux and actual-engine qualification must be repeated. Production consensus parameters, topology, sustained targets and full fault acceptance remain open.

**Decision and record dependencies:** D06-Q01, D06-Q02, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 273–341](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1349–1378](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g02"></a>
## G02 — Consensus safety

**Status: PARTIAL**

**Requirement:** Demonstrate that the production consensus system preserves finalized history and consistent state under its approved Byzantine fault assumptions, partitions, validator changes and recovery.

**Completed work**

- Signed-header checks followed validator sets from trusted local genesis and confirmed application commitments.
- Local quorum loss stopped commitment and issuance; restored nodes agreed on state and supply.
- Latest four-validator compatibility checks preserve commitment and restart behavior.

**Evidence**

- [batch-10/REPORT.md](batch-10/REPORT.md)
- [batch-10/TEST_RESULTS.json](batch-10/TEST_RESULTS.json)
- [batch-10/VALIDATION.json](batch-10/VALIDATION.json)
- [batch-7/APPROVAL.json](batch-7/APPROVAL.json)
- [batch-7/REPORT.md](batch-7/REPORT.md)
- [batch-7/TEST_RESULTS.json](batch-7/TEST_RESULTS.json)
- [batch-7/VALIDATION.json](batch-7/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [batch-9/REPORT.md](batch-9/REPORT.md)
- [batch-9/TEST_RESULTS.json](batch-9/TEST_RESULTS.json)
- [batch-9/VALIDATION.json](batch-9/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/launch-preparation-workstreams/security/REPORT.md](decision-register/launch-preparation-workstreams/security/REPORT.md)
- [decision-register/launch-preparation-workstreams/security/RESULTS.json](decision-register/launch-preparation-workstreams/security/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve the fault model, finality, evidence, retention and synchronization requirements.
- Qualify required fault scenarios, state sync and evidence handling on the frozen candidate.
- Complete independent consensus review and resolve every safety ambiguity.

**Acceptance requirement:** Demonstrate that the production consensus system preserves finalized history and consistent state under its approved Byzantine fault assumptions, partitions, validator changes and recovery.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L03: Review scope, frozen candidate hashes, findings, remediation checks and signed acceptance.
- L09: Seven complete reviewed frozen-candidate simulation records under the existing execution plan.

**Acceptance authority:** Consensus lead and independent security reviewers. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Local agreement checks do not cover the complete production fault model, engine evidence coverage or independent safety acceptance.

**Decision and record dependencies:** D06-Q01, D06-Q02, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1211–1295](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1381–1513](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Use saved package source and executable hashes for credited results. The active implementation tree has later changes; SOURCE_DRIFT.json records the inspected differences. Changed source needs new qualification.

<a id="g03"></a>
## G03 — PQC correctness

**Status: PARTIAL**

**Requirement:** Freeze and qualify the role-specific PQC architecture, key and signature encodings, domain separation, replay protection, key handling and fail-closed behavior. Complete known-answer, cross-implementation and malformed-input checks.

**Completed work**

- Approved profile selects ML-KEM-768 peer establishment, ML-DSA-65 operational signatures and separate SLH-DSA root authorization.
- Profile migration and actual-engine native/browser interoperability have saved results.
- Earlier maintained root checks passed nine normal tests with 133 subcases and three scoped race tests with 35 subcases. Their saved scope predates the development genesis integration.
- Added a verification-only SLH-DSA helper and disabled development genesis consumer. Three root tests and the actual application CLI verified genesis/root receipt/consumed-sequence atomicity and identical restart.
- The new macOS and Linux ARM64 Go engine inventories have zero detected prohibited packages or symbols. Internal review identifies the two provider entries as disabled stubs, declarations and markers; the existing checker still withholds acceptance.
- Development root initialization and restart now hash actual external genesis/release files and reject mismatches before storage opens. Five focused Rust tests and actual root-enabled native/Linux consensus runs passed.
- The selected native application excludes recorded pre-standard PQC and ML-DSA-87 implementation paths. The same valid development-87 signatures are accepted only in the explicit development build and rejected by strict ordinary, general, recovery and sponsor verification. Actual root helper and native consensus/RPC checks passed.
- Implemented and tested typed development upgrade/halt intent admission with atomic pending receipts and root sequence consumption. Production execution is refused. Existing verifier and Linux candidate do not import this component. Selected Linux application and strict SDK artifacts passed the recorded finite algorithm checks.
- Added explicit private scratch selection and unsuccessful-helper exit reporting to the trusted development root consumer. Seven native tests passed, including real-verifier atomic genesis/restart checks. Reviewed the actual root commit/hash/recovery integration boundary; no production action activated.
- Implemented a development SLH-DSA emergency-control verifier with exact helper refusal protocol, distinct infrastructure errors, canonical authorization bytes, bounded inputs and real-helper verification evidence.
- Built the current SLH-DSA Linux helper. Five Linux checks cover actual emergency verification and refusal/infrastructure outcomes. Recorded the approved three-of-five emergency policy with independent human custody inputs still unset. Actual native execution verified three-of-five freeze/resume, threshold refusal, and recorded control signatures on restart.
- Implemented chain/genesis/candidate/epoch/policy-bound emergency controls, bounded signed windows, evidence-bound resume and action-separated SLH-DSA upgrade verification.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/gate-closure-implementation/root/REPORT.md](decision-register/gate-closure-implementation/root/REPORT.md)
- [decision-register/gate-closure-implementation/root/RESULTS.json](decision-register/gate-closure-implementation/root/RESULTS.json)
- [decision-register/pqc-profile/ACCEPTANCE.json](decision-register/pqc-profile/ACCEPTANCE.json)
- [decision-register/pqc-profile/APPROVAL.json](decision-register/pqc-profile/APPROVAL.json)
- [decision-register/pqc-profile/REPORT.md](decision-register/pqc-profile/REPORT.md)
- [decision-register/pqc-profile/TEST_RESULTS.json](decision-register/pqc-profile/TEST_RESULTS.json)
- [decision-register/pqc-profile/VALIDATION.json](decision-register/pqc-profile/VALIDATION.json)
- [decision-register/pqc-transport-integration/ACCEPTANCE.json](decision-register/pqc-transport-integration/ACCEPTANCE.json)
- [decision-register/pqc-transport-integration/EVIDENCE_DIGESTS.json](decision-register/pqc-transport-integration/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-transport-integration/REPORT.md](decision-register/pqc-transport-integration/REPORT.md)
- [decision-register/pqc-transport-integration/TEST_RESULTS.json](decision-register/pqc-transport-integration/TEST_RESULTS.json)
- [decision-register/pqc-transport-integration/VALIDATION.json](decision-register/pqc-transport-integration/VALIDATION.json)
- [decision-register/production-integration/root/RESULTS.json](decision-register/production-integration/root/RESULTS.json)
- [decision-register/production-integration/root/REPORT.md](decision-register/production-integration/root/REPORT.md)
- [decision-register/production-integration/pqc/QUALIFICATION.json](decision-register/production-integration/pqc/QUALIFICATION.json)
- [decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json](decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json)
- [decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json](decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/root/RESULTS.json](decision-register/production-boundary-qualification/root/RESULTS.json)
- [decision-register/production-boundary-qualification/root/REPORT.md](decision-register/production-boundary-qualification/root/REPORT.md)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/crypto/RESULTS.json](decision-register/production-profile-finalization/crypto/RESULTS.json)
- [decision-register/production-profile-finalization/evidence/ARTIFACT_INSPECTION.json](decision-register/production-profile-finalization/evidence/ARTIFACT_INSPECTION.json)
- [decision-register/production-profile-finalization/evidence/APPLICATION_CHECKS.json](decision-register/production-profile-finalization/evidence/APPLICATION_CHECKS.json)
- [decision-register/production-profile-finalization/integration/native01/RUNTIME.json](decision-register/production-profile-finalization/integration/native01/RUNTIME.json)
- [decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md](decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/root/REPORT.md](decision-register/linux-release-authority-qualification/root/REPORT.md)
- [decision-register/linux-release-authority-qualification/root/RESULTS.json](decision-register/linux-release-authority-qualification/root/RESULTS.json)
- [decision-register/linux-release-authority-qualification/linux/RESULTS.json](decision-register/linux-release-authority-qualification/linux/RESULTS.json)
- [decision-register/linux-release-authority-qualification/providers/REPORT.md](decision-register/linux-release-authority-qualification/providers/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/root-helper/REPORT.md](decision-register/native-staging-production-closure/root-helper/REPORT.md)
- [decision-register/native-staging-production-closure/root-helper/RESULTS.json](decision-register/native-staging-production-closure/root-helper/RESULTS.json)
- [decision-register/native-staging-production-closure/review/ROOT_HELPER_REVIEW.json](decision-register/native-staging-production-closure/review/ROOT_HELPER_REVIEW.json)
- [decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md](decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/verifier/REPORT.md](decision-register/emergency-transaction-freeze/verifier/REPORT.md)
- [decision-register/emergency-transaction-freeze/verifier/RESULTS.json](decision-register/emergency-transaction-freeze/verifier/RESULTS.json)
- [decision-register/emergency-transaction-freeze/state/REPORT.md](decision-register/emergency-transaction-freeze/state/REPORT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/linux/REPORT.md](decision-register/emergency-release-staging/linux/REPORT.md)
- [decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json](decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json)
- [decision-register/emergency-release-staging/policy/APPROVAL.json](decision-register/emergency-release-staging/policy/APPROVAL.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/emergency/RESULTS.json](decision-register/emergency-upgrade-execution/emergency/RESULTS.json)
- [decision-register/emergency-upgrade-execution/verifier/RESULTS.json](decision-register/emergency-upgrade-execution/verifier/RESULTS.json)
- [decision-register/emergency-upgrade-execution/review/VALIDATION.json](decision-register/emergency-upgrade-execution/review/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve and qualify production root schemas, manifest contents, custody, role parameters and migration rules. Extend the tested file binding and durable action consumers beyond development genesis.
- Bind known-answer, cross-implementation, malformed-input and side-channel review to the final candidate.
- Resolve the remaining G35 profile/provider boundaries and obtain independent cryptographic acceptance.

**Acceptance requirement:** Freeze and qualify the role-specific PQC architecture, key and signature encodings, domain separation, replay protection, key handling and fail-closed behavior. Complete known-answer, cross-implementation and malformed-input checks.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L02: Frozen source, locks, build environment, executables, configuration templates and provider inventories; independent G35 acceptance. Final production genesis bytes are bound at L11.
- L03: Review scope, frozen candidate hashes, findings, remediation checks and signed acceptance.
- L10: Explicit ceremony authorization, public key validation, custody roles and recovery procedures. Never export private material.

**Acceptance authority:** Cryptography lead and independent cryptographic reviewers. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production schema acceptance, numeric limits, custody, final-candidate cryptographic qualification and independent acceptance remain open. New Rust paths require Linux artifact qualification.

**Decision and record dependencies:** D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 535–613](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1297–1378](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g04"></a>
## G04 — Genesis reproducibility

**Status: PARTIAL**

**Requirement:** Generate deterministic production genesis from approved explicit inputs. Hash the final bytes. Require independent reproduction and identical genesis-hash verification by every validator.

**Completed work**

- Deterministic development generator passed 38 tests.
- Seven retained Rust application checks verified initialization, commitment binding and restart.
- Maintained typed input checks passed 53 tests and ten supported fixture checks. They bind exact native/application bytes, amounts, vesting, validator mappings and public peer pins. Source reconciliation preserves unsupported root, governance and approval fields.
- A public-evidence checker bound 17 populated record references to saved file hashes;17 tests passed. The packet accounts for all 34 existing canonical question IDs and preserves 141 unset reference fields.

**Evidence**

- [batch-6/APPROVAL.json](batch-6/APPROVAL.json)
- [batch-6/REPORT.md](batch-6/REPORT.md)
- [batch-6/TEST_RESULTS.json](batch-6/TEST_RESULTS.json)
- [batch-6/VALIDATION.json](batch-6/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/genesis/DEPENDENCIES.json](decision-register/launch-preparation-workstreams/genesis/DEPENDENCIES.json)
- [decision-register/launch-preparation-workstreams/genesis/REPORT.md](decision-register/launch-preparation-workstreams/genesis/REPORT.md)
- [decision-register/launch-preparation-workstreams/genesis/RESULTS.json](decision-register/launch-preparation-workstreams/genesis/RESULTS.json)
- [decision-register/launch-preparation-workstreams/genesis/evidence/CONSUMER_CHECK.json](decision-register/launch-preparation-workstreams/genesis/evidence/CONSUMER_CHECK.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/production-integration/records/bindings/RESULTS.json](decision-register/production-integration/records/bindings/RESULTS.json)
- [decision-register/production-integration/records/bindings/REPORT.md](decision-register/production-integration/records/bindings/REPORT.md)
- [decision-register/production-integration/records/bindings/CONSUMER_RECONCILIATION/RESULTS.json](decision-register/production-integration/records/bindings/CONSUMER_RECONCILIATION/RESULTS.json)
- [decision-register/production-integration/records/RECORD_CHECK.json](decision-register/production-integration/records/RECORD_CHECK.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/records/RESULTS.json](decision-register/production-boundary-qualification/records/RESULTS.json)
- [decision-register/production-boundary-qualification/records/DECISION_QUEUE.md](decision-register/production-boundary-qualification/records/DECISION_QUEUE.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Complete the eight production records, network identity and 11 unset runtime bindings.
- Complete the remaining typed consumer adapters and reviewed production-generation support.
- Generate and independently reproduce permanent genesis; record each validator verification and authorized acceptance.

**Acceptance requirement:** Generate deterministic production genesis from approved explicit inputs. Hash the final bytes. Require independent reproduction and identical genesis-hash verification by every validator.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L10: Explicit ceremony authorization, public key validation, custody roles and recovery procedures. Never export private material.
- L11: Exact serialized production genesis bytes, public keys, file hashes and approved protocol commitments; independent verification; completed final candidate/key/genesis record bindings for acceptance before L13.

**Acceptance authority:** Release lead, independent genesis verifier and validator operators. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Required production genesis values, typed consumers and accepted records remain incomplete. File-hash binding does not authenticate an approval or authorize production generation.

**Decision and record dependencies:** D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D10-Q01, D10-Q02, D10-Q03, D13-Q01, D13-Q02, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 615–679](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Latest selected source and saved snapshots were verified in decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json. Earlier results remain bound to their original saved artifacts. The new builds and runtime checks are local qualification, not a production release freeze.

<a id="g05"></a>
## G05 — Genesis allocation correctness

**Status: PARTIAL**

**Requirement:** Reconcile every production genesis allocation to the approved supply in integer base units. Verify recipients, custody, vesting, delegation, validator funding and initial DRT bootstrap.

**Completed work**

- Approved DGT allocation is one billion tokens, 10^15 base units at six decimals, with shares 30/20/15/15/20.
- Genesis preparation checks bucket totals, unique accounts, explicit vesting and funded stake.
- Eight record packets preserve approved totals without inventing beneficiaries or initial mint authority.
- Added typed cross-file checks for supplied allocation amounts, vesting, funded delegations and operator mappings. The development fixture passed supported checks without changing approved economics or granting recipient/custody acceptance.

**Evidence**

- [batch-6/APPROVAL.json](batch-6/APPROVAL.json)
- [batch-6/REPORT.md](batch-6/REPORT.md)
- [batch-6/TEST_RESULTS.json](batch-6/TEST_RESULTS.json)
- [batch-6/VALIDATION.json](batch-6/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/genesis/DEPENDENCIES.json](decision-register/launch-preparation-workstreams/genesis/DEPENDENCIES.json)
- [decision-register/launch-preparation-workstreams/genesis/REPORT.md](decision-register/launch-preparation-workstreams/genesis/REPORT.md)
- [decision-register/launch-preparation-workstreams/genesis/RESULTS.json](decision-register/launch-preparation-workstreams/genesis/RESULTS.json)
- [decision-register/launch-preparation-workstreams/genesis/evidence/CONSUMER_CHECK.json](decision-register/launch-preparation-workstreams/genesis/evidence/CONSUMER_CHECK.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/production-integration/records/bindings/RESULTS.json](decision-register/production-integration/records/bindings/RESULTS.json)
- [decision-register/production-integration/records/bindings/REPORT.md](decision-register/production-integration/records/bindings/REPORT.md)
- [decision-register/production-integration/records/REPORT.md](decision-register/production-integration/records/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve DGT initial issuance and DRT bootstrap policy.
- Supply verified recipients, vesting schedules, initial delegations, treasury and custody acceptances.
- Reconcile the final generated allocation ledger and independently verify the genesis hash.

**Acceptance requirement:** Reconcile every production genesis allocation to the approved supply in integer base units. Verify recipients, custody, vesting, delegation, validator funding and initial DRT bootstrap.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L01: Verified preparable fields; approved input policy and role assignments. Final candidate, production-key and genesis bindings remain pending until L11-L13.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L10: Explicit ceremony authorization, public key validation, custody roles and recovery procedures. Never export private material.
- L11: Exact serialized production genesis bytes, public keys, file hashes and approved protocol commitments; independent verification; completed final candidate/key/genesis record bindings for acceptance before L13.

**Acceptance authority:** Economics lead, custodians and independent genesis verifier. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Exact beneficiary/custody records and initial issuance decisions remain open. A full-supply fixture does not approve minting all DGT at genesis.

**Decision and record dependencies:** D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D13-Q01, D13-Q02. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 615–679](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g06"></a>
## G06 — Binary reproducibility

**Status: PARTIAL**

**Requirement:** Freeze the full production source and artifact set, build environment, locks, checksums, container image, dependency inventory and release manifest. Reproduce identical production binaries on an independent clean machine.

**Completed work**

- Release tooling verified 1,121 archived source files, four Go lock files and manifests for two earlier executables. Sixteen tests and seven transition rejection subcases passed.
- Earlier macOS arm64 and Linux arm64 engine rebuilds match locally within their saved candidate scope.
- Two fresh-target wallet WASM builds match each other and all four maintained generated assets.
- New macOS and Linux ARM64 Go IPC engine artifacts each passed a byte-identical same-host rebuild. The standalone native Hyper adapter build passed with 30 locked registry packages and 1,938 source files checked. The updated native Rust application also built successfully.
- Built and retained the exact Linux ARM64 application ELF with the explicit default feature set. Recorded locked dependency resolution, toolchain identity, 365 unchanged source/Cargo hashes, exported artifact hash and linked-provider hashes. This was not an independent or byte-identical application rebuild.
- The selected application and adapter built for Linux x86_64 with pinned Rust 1.88.0 and 385 unchanged source inputs. Native node and SDK registry files were verified against locked archives; exact executables and library closures are retained.
- The selected native build records exact source and compiler identities and reuses pinned freshly compiled/tested LZ4+Snappy archives. Native artifact inspection and compatibility compilation passed. This is not independent or clean production release reproduction.
- Built and inspected Linux x86_64 release application, adapter and strict SDK CLI. The CLI builds from a verified standalone source archive. Retained exact source, dependency, native archive, artifact and library hashes. Source archives repeat identically; the adapter matches the previous local artifact. This is not independent full-release reproduction.
- Preserved the prior sealed Linux release and verified 1917 retained files. Ran the unchanged packaged strict CLI natively on Hetzner. Bound the new helper source to local test evidence; no rebuilt Linux application or independent reproduction is claimed.
- Rebuilt the current Linux x86_64 application and updated root helper from pinned source and dependency inputs. Inspected exact artifacts and preserved prior evidence. The pinned application/helper ran in the native four-node fixture.
- Pinned 429 current source files, preserved the prior sealed artifact evidence and recorded commands and test-source hashes for the changed Rust implementation.

**Evidence**

- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json](decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json)
- [decision-register/launch-preparation-workstreams/release/REPORT.md](decision-register/launch-preparation-workstreams/release/REPORT.md)
- [decision-register/launch-preparation-workstreams/release/RESULTS.json](decision-register/launch-preparation-workstreams/release/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/ACCEPTANCE.json](decision-register/pqc-build-boundary/ACCEPTANCE.json)
- [decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json](decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-build-boundary/REPORT.md](decision-register/pqc-build-boundary/REPORT.md)
- [decision-register/pqc-build-boundary/TEST_RESULTS.json](decision-register/pqc-build-boundary/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/VALIDATION.json](decision-register/pqc-build-boundary/VALIDATION.json)
- [decision-register/production-integration/pqc/QUALIFICATION.json](decision-register/production-integration/pqc/QUALIFICATION.json)
- [decision-register/production-integration/pqc/http-adapter/RESULTS.json](decision-register/production-integration/pqc/http-adapter/RESULTS.json)
- [decision-register/production-integration/root/RESULTS.json](decision-register/production-integration/root/RESULTS.json)
- [decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json](decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/evidence/BUILD_TOOLCHAIN.json](decision-register/production-integration/linux/retry/evidence/BUILD_TOOLCHAIN.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json](decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/linux-x86/REPORT.md](decision-register/production-boundary-qualification/linux-x86/REPORT.md)
- [decision-register/production-boundary-qualification/rust/RESULTS.json](decision-register/production-boundary-qualification/rust/RESULTS.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/evidence/PINNED_NATIVE_ARCHIVES.json](decision-register/production-profile-finalization/evidence/PINNED_NATIVE_ARCHIVES.json)
- [decision-register/production-profile-finalization/evidence/BUILT_ARTIFACT.json](decision-register/production-profile-finalization/evidence/BUILT_ARTIFACT.json)
- [decision-register/production-profile-finalization/evidence/LOCAL_TOOLS.json](decision-register/production-profile-finalization/evidence/LOCAL_TOOLS.json)
- [decision-register/production-profile-finalization/evidence/APPLICATION_CHECKS.json](decision-register/production-profile-finalization/evidence/APPLICATION_CHECKS.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/linux/REPORT.md](decision-register/linux-release-authority-qualification/linux/REPORT.md)
- [decision-register/linux-release-authority-qualification/linux/RESULTS.json](decision-register/linux-release-authority-qualification/linux/RESULTS.json)
- [decision-register/linux-release-authority-qualification/sdk/REPORT.md](decision-register/linux-release-authority-qualification/sdk/REPORT.md)
- [decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md](decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/root-helper/REPORT.md](decision-register/native-staging-production-closure/root-helper/REPORT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/linux/REPORT.md](decision-register/emergency-release-staging/linux/REPORT.md)
- [decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json](decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json)
- [decision-register/emergency-release-staging/linux/VALIDATION.json](decision-register/emergency-release-staging/linux/VALIDATION.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)
- [decision-register/emergency-upgrade-execution/review/SOURCE_SNAPSHOT.json](decision-register/emergency-upgrade-execution/review/SOURCE_SNAPSHOT.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Freeze all production binaries, Rust application, bridge, wallet, SDK, container and dependencies with exact hashes.
- Reproduce the full set on an independently controlled clean builder and execute the intended Linux targets.
- Complete dependency/provider review, provenance and authorized release acceptance.

**Acceptance requirement:** Freeze the full production source and artifact set, build environment, locks, checksums, container image, dependency inventory and release manifest. Reproduce identical production binaries on an independent clean machine.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L02: Frozen source, locks, build environment, executables, configuration templates and provider inventories; independent G35 acceptance. Final production genesis bytes are bound at L11.
- L12: Authenticated approved distribution, signer/revocation checks and per-host exact artifact/config/genesis equality.

**Acceptance authority:** Release lead and independent build reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Changed Rust code has local executable evidence. Current Linux and actual-engine qualification must be repeated. Full production distribution freeze, independent clean reproduction, provider review and release acceptance remain open.

**Decision and record dependencies:** D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 719–755](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1297–1347](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g07"></a>
## G07 — Validator deployment

**Status: PARTIAL**

**Requirement:** Deploy the production validator stack repeatedly from clean approved servers using documented automation. Configure security, genesis, keys, peers, monitoring, logs and backups; synchronize and join consensus.

**Completed work**

- Three historical Hetzner candidate assets and preparation records are preserved.
- Maintained supervisor, systemd unit and manifest template now cover the engine, bridge and application.
- Fifteen Linux supervisor tests, systemd syntax verification and three Linux engine/bridge startup-refusal checks passed. Actual macOS supervision passed 19 assertions.
- Completed an actual Linux ARM64 systemd run with four restricted service instances, the Rust application, bridge and retained TCP RPC engine. Thirty-six checks passed, including a signed Send and restart. Mainnet host configuration remained unchanged.
- Four actual x86_64 validators ran the combined root-enabled application/IPC/Hyper stack under UID 10001 and passed 37 checks. This is process-supervisor evidence, separate from the earlier ARM 64 systemd run.
- Ran the selected Linux stack under four actual systemd units in an isolated emulated container. The maintained unit and supervisor were unchanged. Service restrictions, consensus participation and local restart evidence are recorded with fixture and emulator limits.
- Inspected three known Hetzner hosts without changing persistent configuration. Ran two bounded native CLI startup commands under transient isolated services on one shared host. Existing workloads and cleanup were verified; the full validator suite did not run.
- Extended service supervision with explicit pinned emergency verifier configuration and private scratch validation. All 56 service tests passed, including twelve new checks. Selected Falkenstein for bounded temporary native staging under the user website-preservation constraint. Fifty native lifecycle assertions passed inside one transient unit with four supervisors. Scoped cleanup and protected service/listener preservation passed.
- Prepared current emergency and upgrade application changes for the next native qualification. No remote deployment occurred; the prior service-preservation record remains unchanged.
- Candidate 15 development-only native admission passed on four nodes. Both cancellation and both owner-loss cases passed. The two deadline cases did not qualify. The controlled SIGSTOP probe was denied before injection; all case cleanup and protected-host afterchecks passed.
- Three additional native normal-admission trials produced 24 exact post-GO helper exit samples. Each trial passed owned cleanup and Falkenstein/Ashburn preservation checks. The samples do not qualify either deadline case.

**Evidence**

- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/ordinary-client-compatibility/ACCEPTANCE.json](decision-register/ordinary-client-compatibility/ACCEPTANCE.json)
- [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json)
- [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md)
- [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json)
- [decision-register/ordinary-client-compatibility/VALIDATION.json](decision-register/ordinary-client-compatibility/VALIDATION.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/REPORT.md](decision-register/production-integration/linux/retry/REPORT.md)
- [decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json](decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json)
- [decision-register/production-integration/records/ROLE_NOMINATIONS.json](decision-register/production-integration/records/ROLE_NOMINATIONS.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-boundary-qualification/integration/linux02/RUNTIME.json](decision-register/production-boundary-qualification/integration/linux02/RUNTIME.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/RESULTS.json](decision-register/linux-release-authority-qualification/services/RESULTS.json)
- [decision-register/linux-release-authority-qualification/services/SELECTED_COMPONENT_INPUTS.json](decision-register/linux-release-authority-qualification/services/SELECTED_COMPONENT_INPUTS.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/staging/RESULTS.json](decision-register/native-staging-production-closure/staging/RESULTS.json)
- [decision-register/native-staging-production-closure/staging/native-smoke/RESULT.json](decision-register/native-staging-production-closure/staging/native-smoke/RESULT.json)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/service/REPORT.md](decision-register/emergency-release-staging/service/REPORT.md)
- [decision-register/emergency-release-staging/service/TEST_RESULTS.json](decision-register/emergency-release-staging/service/TEST_RESULTS.json)
- [decision-register/emergency-release-staging/staging/REPORT.md](decision-register/emergency-release-staging/staging/REPORT.md)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-admission-native-v1/ADMISSION_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-admission-native-v1/ADMISSION_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-cancel-native-v1/TRIAL_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-cancel-native-v1/TRIAL_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-application-cancel-native-v1/TRIAL_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-application-cancel-native-v1/TRIAL_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-owner-loss-trace-v2-native-v1/TRIAL_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-owner-loss-trace-v2-native-v1/TRIAL_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-application-owner-loss-trace-v2-native-v1/TRIAL_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-application-owner-loss-trace-v2-native-v1/TRIAL_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-timing-diagnostic-v1/DIAGNOSTIC_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-timing-diagnostic-v1/DIAGNOSTIC_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-deadline-stall-native-v1/FAILURE_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-deadline-stall-native-v1/FAILURE_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-method-assessment-2026-09-24/NATIVE_MEASUREMENT_2026-09-24.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-method-assessment-2026-09-24/NATIVE_MEASUREMENT_2026-09-24.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve operators, hosts, topology, service roles, funding, custody and the verified root-helper execution path.
- Qualify the exact accepted stack on approved x86_64 staging hosts with monitoring, logs and backups.
- Demonstrate repeated clean provisioning, secure paths, synchronization and consensus participation under named operators.
- Qualify both deadline cases with a reviewed test method under the effective service policy; run a common seven-case aggregate and the full 40 service checks on one current candidate.

**Acceptance requirement:** Deploy the production validator stack repeatedly from clean approved servers using documented automation. Configure security, genesis, keys, peers, monitoring, logs and backups; synchronize and join consensus.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L01: Verified preparable fields; approved input policy and role assignments. Final candidate, production-key and genesis bindings remain pending until L11-L13.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.
- L12: Authenticated approved distribution, signer/revocation checks and per-host exact artifact/config/genesis equality.

**Acceptance authority:** SRE lead and named validator operators. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Changed Rust code has local executable evidence. Current Linux and actual-engine qualification must be repeated. Approved production topology, operators, custody, repeated clean provisioning, monitoring and backups remain open.

**Decision and record dependencies:** D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 757–801](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g08"></a>
## G08 — Validator recovery

**Status: PARTIAL**

**Requirement:** Recover and replace validators through documented procedures while preserving finalized state, key custody and exclusive signing. Meet approved recovery objectives and rejoin consensus safely.

**Completed work**

- Five four-validator local recovery/replacement drills passed 261 checks.
- All five existing receipts persisted on all validators and new blocks committed after restart.
- Maintained supervision demonstrated actual macOS stop/start/restart behavior.
- The actual four-service Linux ARM64 run preserved all peer identities and the committed receipt on every node after restart. Heights advanced from four to six. The disposable same-host test does not establish off-host replacement or stale-signing safety.
- The combined x86_64 run preserved four peer identities, root-required database state and matching receipts after restart. Equal-height application anchors matched across the four nodes.
- Verified same-fixture graceful restart and manual recovery after one selected engine crash under systemd. The fixture retains signing state and committed receipts. This does not qualify off-host recovery or stale signer restoration.
- Normal frozen and resumed restarts preserved control history, peer identities and the persistent upgrade hold. Signing-state positions and same-position digests were checked.
- Local actual-database checks preserve emergency and upgrade state across failed commit, lost acknowledgement and restart. Public custody intake now validates required evidence structure.

**Evidence**

- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json](decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json](decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-boundary-qualification/integration/linux02/RUNTIME.json](decision-register/production-boundary-qualification/integration/linux02/RUNTIME.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/RESULTS.json](decision-register/linux-release-authority-qualification/services/RESULTS.json)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)
- [decision-register/emergency-upgrade-execution/custody/RESULTS.json](decision-register/emergency-upgrade-execution/custody/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Qualify remaining Linux failure/recovery scenarios and off-host replacement on approved x86_64 staging.
- Demonstrate exclusive signing and safe handling of stale or conflicting signer state.
- Measure approved recovery objectives and obtain operator acceptance.

**Acceptance requirement:** Recover and replace validators through documented procedures while preserving finalized state, key custody and exclusive signing. Meet approved recovery objectives and rejoin consensus safely.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L06: Signer exclusivity, backup epoch, anti-double-sign state, state compatibility and reviewed source/target upgrade pair.

**Acceptance authority:** Validator operators, SRE lead and independent recovery reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** New Rust behavior needs native requalification. Off-host recovery, signing-state conflicts, accepted custodians, measured recovery objectives and operator acceptance remain open.

**Decision and record dependencies:** D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 953–1025](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g09"></a>
## G09 — State recovery

**Status: PARTIAL**

**Requirement:** Restore application and consensus state after the required failures. Verify finalized history, balances, supply, stake, receipts and signing safety after every recovery.

**Completed work**

- Atomic application commits and retry/reopen checks cover balances, fees, nonces, receipts and authority.
- Stopped backup restored 79 files and exact committed state; five receipts persisted across four validators.
- Latest supervision checks confirm restart commitment, without adding database-restore coverage.
- Added durable emergency state and immutable receipts to the existing atomic commit batch. Development recovery validates control history; application restart also repeats cryptographic verification.

**Evidence**

- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json](decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-transaction-freeze/integration/RESULTS.json](decision-register/emergency-transaction-freeze/integration/RESULTS.json)
- [decision-register/emergency-transaction-freeze/state/REPORT.md](decision-register/emergency-transaction-freeze/state/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Qualify corruption, server/disk loss, snapshot restore and off-host recovery on the complete frozen Linux stack.
- Verify finalized history, state hashes, supply, stake and signer exclusivity for every required scenario.
- Meet approved recovery and retention objectives and record reviewer acceptance.

**Acceptance requirement:** Restore application and consensus state after the required failures. Verify finalized history, balances, supply, stake, receipts and signing safety after every recovery.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L06: Signer exclusivity, backup epoch, anti-double-sign state, state compatibility and reviewed source/target upgrade pair.

**Acceptance authority:** Storage/SRE leads and independent recovery reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** A stopped same-host restore before later signing does not cover stale snapshots, corruption, physical loss or production recovery objectives.

**Decision and record dependencies:** D06-Q01, D06-Q02, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 953–1025](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json. Emergency freeze and resume qualification is limited to the recorded development profile and test environment. Production activation and G35 acceptance remain blocked.

<a id="g10"></a>
## G10 — PQC transaction lifecycle

**Status: PARTIAL**

**Requirement:** Qualify the complete PQC transaction lifecycle from account authority and exact signing bytes through admission, commitment, fees, receipts and persistence. Reject wrong keys, networks, replay and stale authorization.

**Completed work**

- All twelve ordinary actions share current-key, generation, debit-owner, fee and atomic storage checks. The local runtime acceptance map links 20 requirements to 69 named tests.
- Recovery signing, sponsored fees and all-path protections have retained local evidence.
- Native and browser modules passed actual-engine transaction, fee, nonce, replay and restart checks; latest engine run adds 154 assertions and six transport checks.
- The native client passed submission, commitment, receipt and restart checks through four actual engines and four Hyper adapters. The 160-check result uses the retained application; the new root consumer has separate application-CLI evidence. These results are not a combined root-enabled network qualification.
- A native CLI submitted a signed Send through the actual Linux engine RPC. It stayed uncommitted without quorum, committed after quorum returned and produced matching receipts on four nodes before and after service restart. This run uses the retained TCP RPC engine.
- The selected local ordinary CLI reused unchanged command logic, rejected HTTPS/external endpoints and proxies, and passed command/SDK checks plus native and Linux consensus receipt/restart paths.
- Exercised the extracted strict Linux CLI through the actual adapter, IPC bridge, consensus engine and application. Verified submission, no-quorum behavior, commitment, matching receipts and restart persistence in the isolated service fixture.
- Integrated development emergency controls with ordinary transaction admission and execution. Freeze controls take priority throughout their block; frozen user actions cannot consume fees or nonces. Resume permits user actions only from the next block.
- The actual Linux client submitted through the native consensus RPC path. A fresh current-nonce transaction was refused specifically during freeze, then committed after resume with one nonce increment and matching replica receipts.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json)
- [decision-register/ordinary-signing-implementation/REPORT.md](decision-register/ordinary-signing-implementation/REPORT.md)
- [decision-register/ordinary-signing-implementation/TEST_RESULTS.json](decision-register/ordinary-signing-implementation/TEST_RESULTS.json)
- [decision-register/ordinary-signing-implementation/VALIDATION.json](decision-register/ordinary-signing-implementation/VALIDATION.json)
- [decision-register/pqc-build-boundary/ACCEPTANCE.json](decision-register/pqc-build-boundary/ACCEPTANCE.json)
- [decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json](decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-build-boundary/REPORT.md](decision-register/pqc-build-boundary/REPORT.md)
- [decision-register/pqc-build-boundary/TEST_RESULTS.json](decision-register/pqc-build-boundary/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/VALIDATION.json](decision-register/pqc-build-boundary/VALIDATION.json)
- [decision-register/recovery-acceptance-followup/ACCEPTANCE_STATUS.json](decision-register/recovery-acceptance-followup/ACCEPTANCE_STATUS.json)
- [decision-register/recovery-acceptance-followup/REPORT.md](decision-register/recovery-acceptance-followup/REPORT.md)
- [decision-register/recovery-acceptance-followup/TEST_RESULTS.json](decision-register/recovery-acceptance-followup/TEST_RESULTS.json)
- [decision-register/recovery-acceptance-followup/VALIDATION.json](decision-register/recovery-acceptance-followup/VALIDATION.json)
- [decision-register/recovery-fee-implementation/ACCEPTANCE_STATUS.json](decision-register/recovery-fee-implementation/ACCEPTANCE_STATUS.json)
- [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json)
- [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md)
- [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json)
- [decision-register/recovery-fee-implementation/VALIDATION.json](decision-register/recovery-fee-implementation/VALIDATION.json)
- [decision-register/recovery-signing/APPROVAL.json](decision-register/recovery-signing/APPROVAL.json)
- [decision-register/recovery-signing/REPORT.md](decision-register/recovery-signing/REPORT.md)
- [decision-register/recovery-signing/TEST_RESULTS.json](decision-register/recovery-signing/TEST_RESULTS.json)
- [decision-register/recovery-signing/VALIDATION.json](decision-register/recovery-signing/VALIDATION.json)
- [decision-register/production-integration/pqc/ipc-runtime/RESULTS.json](decision-register/production-integration/pqc/ipc-runtime/RESULTS.json)
- [decision-register/production-integration/pqc/ipc-runtime/REPORT.md](decision-register/production-integration/pqc/ipc-runtime/REPORT.md)
- [decision-register/production-integration/root/RESULTS.json](decision-register/production-integration/root/RESULTS.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json](decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json)
- [decision-register/production-integration/linux/retry/evidence/COMMITTED_RECEIPT.json](decision-register/production-integration/linux/retry/evidence/COMMITTED_RECEIPT.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/RESULTS.json](decision-register/linux-release-authority-qualification/services/RESULTS.json)
- [decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md](decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-transaction-freeze/integration/RESULTS.json](decision-register/emergency-transaction-freeze/integration/RESULTS.json)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Qualify the final candidate with production role/fee/recovery inputs and current maintained clients.
- Complete independent committed-state and receipt inclusion verification.
- Complete account enrollment/migration, recovery custody and hosted end-to-end acceptance.

**Acceptance requirement:** Qualify the complete PQC transaction lifecycle from account authority and exact signing bytes through admission, commitment, fees, receipts and persistence. Reject wrong keys, networks, replay and stale authorization.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Runtime and wallet leads; independent transaction reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Native selected-client execution passed within the disposable fixture. Production enrollment/recovery custody, final role/fee inputs, hosted acceptance and independent state/receipt inclusion proofs remain open.

**Decision and record dependencies:** D04-Q01, D04-Q02, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 535–613](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 803–857](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json. Package reports identify exact build, platform, test and staging scope. Approved policy does not grant production activation or G35 acceptance.

<a id="g11"></a>
## G11 — Wallet reliability

**Status: PARTIAL**

**Requirement:** Qualify wallet creation, encrypted key storage, persistence, import/recovery, signing, fees, nonces, failures, RPC reconnect, device/application restart and upgrades. Prevent silent loss of asset access.

**Completed work**

- Actual browser signing, commitment, nonce/fee changes and four-RPC receipt agreement were observed in the earlier exact-engine run.
- Track 3 passed 48 module tests and 29 browser harness cases, including session loss after reload.
- Latest batch integrated the regressions into maintained source and reproduced all four generated wallet assets.
- The underlying ordinary SDK and explicit local HTTP CLI passed focused signing/client checks and combined consensus receipt/restart tests. No new hosted-browser qualification is claimed.
- The strict Linux SDK bundle now has native help/version startup evidence on Hetzner in addition to the prior local bundle and service tests. This queue did not run the hosted wallet or qualify durable wallet custody.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-3/REPORT.md](decision-register/parallel-tracks-20260912/track-3/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-3/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-3/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/ACCEPTANCE.json](decision-register/pqc-build-boundary/ACCEPTANCE.json)
- [decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json](decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-build-boundary/REPORT.md](decision-register/pqc-build-boundary/REPORT.md)
- [decision-register/pqc-build-boundary/TEST_RESULTS.json](decision-register/pqc-build-boundary/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/VALIDATION.json](decision-register/pqc-build-boundary/VALIDATION.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/decisions/REPORT.md](decision-register/native-staging-production-closure/decisions/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Complete the full wallet interface and encrypted custody/import/recovery matrix, including device restart and upgrade.
- Qualify the frozen hosted assets with independent RPC trust, state and receipt inclusion evidence.
- Repeat negative, reconnect, reload and chain-restart cases with the final maintained candidate.

**Acceptance requirement:** Qualify wallet creation, encrypted key storage, persistence, import/recovery, signing, fees, nonces, failures, RPC reconnect, device/application restart and upgrades. Prevent silent loss of asset access.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Wallet lead, custody reviewer and independent client reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Native strict CLI startup is qualified within its bounded scope. Durable wallet custody, full application-proof interfaces, accepted public endpoint trust and frozen hosted-wallet acceptance remain open.

**Decision and record dependencies:** D04-Q01, D04-Q02, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 803–857](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json. The root-helper change has native local tests; the prior Linux application predates it. Native Hetzner qualification in this queue covers strict CLI startup only. No full native validator staging or production acceptance is claimed.

<a id="g12"></a>
## G12 — DRT transfers

**Status: PARTIAL**

**Requirement:** Execute signed DRT transfers through the frozen network. Verify authorization, balances, fees, failures, replay protection and restart persistence; reconcile all balance changes to global DRT supply.

**Completed work**

- Shared six-decimal units and ordinary signed Send execution have local checks.
- Actual-engine native/browser runs check successful transfers, charged failures, out-of-gas, nonce changes, receipts and restart.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve and configure initial DRT funding, fees, custody and transfer restrictions.
- Run DRT transfer, insufficient-funds, failure, duplicate and restart cases on the frozen network.
- Reconcile every transfer and fee with the production supply ledger and obtain independent acceptance.

**Acceptance requirement:** Execute signed DRT transfers through the frozen network. Verify authorization, balances, fees, failures, replay protection and restart persistence; reconcile all balance changes to global DRT supply.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Runtime/economics leads and independent accounting reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production DRT bootstrap, fee calibration and final economic qualification remain open.

**Decision and record dependencies:** D04-Q01, D04-Q02, D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g13"></a>
## G13 — DGT transfers

**Status: PARTIAL**

**Requirement:** Execute signed DGT transfers through the frozen network. Enforce ownership and vesting restrictions. Verify balances, fees, failures, replay protection and persistence against global DGT supply.

**Completed work**

- Shared DGT units and cap checks are implemented.
- Ordinary execution checks actual debit owners and rejects locked stake-only funds for unrestricted transfers.

**Evidence**

- [batch-6/APPROVAL.json](batch-6/APPROVAL.json)
- [batch-6/REPORT.md](batch-6/REPORT.md)
- [batch-6/TEST_RESULTS.json](batch-6/TEST_RESULTS.json)
- [batch-6/VALIDATION.json](batch-6/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/genesis/REPORT.md](decision-register/launch-preparation-workstreams/genesis/REPORT.md)
- [decision-register/launch-preparation-workstreams/genesis/RESULTS.json](decision-register/launch-preparation-workstreams/genesis/RESULTS.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Complete production DGT allocation, vesting and initial mint decisions.
- Qualify transferable and locked balances through maintained clients and the frozen chain, including failure and restart.
- Reconcile transfers, custody and supply and record independent acceptance.

**Acceptance requirement:** Execute signed DGT transfers through the frozen network. Enforce ownership and vesting restrictions. Verify balances, fees, failures, replay protection and persistence against global DGT supply.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Runtime/economics leads and independent accounting reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production beneficiaries, vesting inputs and mint authority remain incomplete; local generic transfer tests do not qualify the final DGT lifecycle.

**Decision and record dependencies:** D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g14"></a>
## G14 — DRT supply accounting

**Status: PARTIAL**

**Requirement:** Define DRT supply authority and parameters. Prove that minted minus burned amounts equal supply changes, rewards reconcile to issuance, and fees reconcile to custody/distribution/burning under all supported outcomes.

**Completed work**

- Approved epoch scheduling splits each budget 40/30/30 with a separate remainder reserve. Only issued block amounts enter supply.
- Atomic writes bind controller state, observations, issuance, pools, fees, claims and receipts. Retry and restart tests prevent duplicate issuance.
- Actual-engine tests retain local supply agreement through quorum loss and restart.

**Evidence**

- [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json)
- [batch-6/issuance-timing/REPORT.md](batch-6/issuance-timing/REPORT.md)
- [batch-6/issuance-timing/TEST_RESULTS.json](batch-6/issuance-timing/TEST_RESULTS.json)
- [batch-6/issuance-timing/VALIDATION.json](batch-6/issuance-timing/VALIDATION.json)
- [batch-7/APPROVAL.json](batch-7/APPROVAL.json)
- [batch-7/REPORT.md](batch-7/REPORT.md)
- [batch-7/TEST_RESULTS.json](batch-7/TEST_RESULTS.json)
- [batch-7/VALIDATION.json](batch-7/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve controller version/calibration, epoch N, observations, initial supply, burn rules and pause/resume or archival behavior.
- Connect accepted recipient and custody records to final configuration.
- Run complete invariant, edge, failed-write and recovery checks on the frozen production candidate.

**Acceptance requirement:** Define DRT supply authority and parameters. Prove that minted minus burned amounts equal supply changes, rewards reconcile to issuance, and fees reconcile to custody/distribution/burning under all supported outcomes.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Economics/runtime leads and independent supply reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production controller/observation configuration, bootstrap and burn authority remain unresolved.

**Decision and record dependencies:** D01-Q01, D01-Q02, D02-Q01, D02-Q02, D03-Q01, D04-Q01, D04-Q02, D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g15"></a>
## G15 — DGT supply accounting

**Status: PARTIAL**

**Requirement:** Enforce the approved DGT cap and exact integer supply reconciliation across genesis, transfers, vesting, staking, unbonding, penalties and withdrawals. Define initial mint and any future mint/burn authority.

**Completed work**

- One billion DGT and six decimals are approved; integer allocation totals reconcile to 10^15 base units.
- Lifecycle and penalty custody tests preserve funded principal and separate penalty reserve.
- Genesis preparation checks allocation totals, funded stake and vesting input consistency.

**Evidence**

- [batch-6/APPROVAL.json](batch-6/APPROVAL.json)
- [batch-6/REPORT.md](batch-6/REPORT.md)
- [batch-6/TEST_RESULTS.json](batch-6/TEST_RESULTS.json)
- [batch-6/VALIDATION.json](batch-6/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [batch-9/REPORT.md](batch-9/REPORT.md)
- [batch-9/TEST_RESULTS.json](batch-9/TEST_RESULTS.json)
- [batch-9/VALIDATION.json](batch-9/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/genesis/REPORT.md](decision-register/launch-preparation-workstreams/genesis/REPORT.md)
- [decision-register/launch-preparation-workstreams/genesis/RESULTS.json](decision-register/launch-preparation-workstreams/genesis/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve full-versus-partial initial issuance and later mint/burn authority.
- Supply exact allocations and vesting records; qualify locked-principal liability.
- Reconcile every production account, pool, stake and reserve through lifecycle, failure and restart.

**Acceptance requirement:** Enforce the approved DGT cap and exact integer supply reconciliation across genesis, transfers, vesting, staking, unbonding, penalties and withdrawals. Define initial mint and any future mint/burn authority.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Economics/runtime leads and independent supply reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** The cap is approved, but genesis mint scope, later authority and complete production custody inputs are not accepted.

**Decision and record dependencies:** D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g16"></a>
## G16 — Staking

**Status: PARTIAL**

**Requirement:** Qualify funded staking, exact bonded voting power, self-bond eligibility, validator participation and reward entitlement. Preserve principal and vesting restrictions across H+2 changes, failures and restart.

**Completed work**

- Batch 8 implements one power unit per effective bonded uDGT, funded self-bond checks and H+2 activation.
- Local six-node lifecycle checks cover registration, delegation, unbonding, key rotation and exit.
- Ordinary runtime integrates signed stake actions with fees and atomic persistence.

**Evidence**

- [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json)
- [batch-6/integration-followup/REPORT.md](batch-6/integration-followup/REPORT.md)
- [batch-6/integration-followup/TEST_RESULTS.json](batch-6/integration-followup/TEST_RESULTS.json)
- [batch-6/integration-followup/VALIDATION.json](batch-6/integration-followup/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve self-bond, active-set limits, operator registry and final eligibility parameters.
- Complete vesting and locked-principal rules and final production records.
- Qualify the complete signed staking lifecycle and supply reconciliation on the frozen network.

**Acceptance requirement:** Qualify funded staking, exact bonded voting power, self-bond eligibility, validator participation and reward entitlement. Preserve principal and vesting restrictions across H+2 changes, failures and restart.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Validator/economics leads and independent lifecycle reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production stake thresholds, operators and locked-principal qualification remain open.

**Decision and record dependencies:** D02-Q01, D02-Q02, D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g17"></a>
## G17 — Delegation

**Status: PARTIAL**

**Requirement:** Qualify owner-authorized delegation to eligible validators with exact power and rewards, zero staking-pool commission, preserved vesting restrictions and atomic custody changes.

**Completed work**

- Approved rules credit funded bonded stake to its owner and count self-stake once. Liquid and unbonding funds have no power.
- H+2 delegation changes passed local six-node lifecycle checks; signed RewardBond uses common fee/storage integration.

**Evidence**

- [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json)
- [batch-6/integration-followup/REPORT.md](batch-6/integration-followup/REPORT.md)
- [batch-6/integration-followup/TEST_RESULTS.json](batch-6/integration-followup/TEST_RESULTS.json)
- [batch-6/integration-followup/VALIDATION.json](batch-6/integration-followup/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Bind accepted operators, beneficiary permissions and production admission values.
- Qualify delegation concentration, failure, replay and restart behavior with final custody and authority.
- Reconcile delegated positions, rewards and global supply on the frozen candidate.

**Acceptance requirement:** Qualify owner-authorized delegation to eligible validators with exact power and rewards, zero staking-pool commission, preserved vesting restrictions and atomic custody changes.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Validator/economics leads and independent lifecycle reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Final operator/beneficiary records and production delegation qualification remain incomplete.

**Decision and record dependencies:** D02-Q01, D02-Q02, D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D11-Q01, D11-Q02, D11-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g18"></a>
## G18 — Undelegation

**Status: PARTIAL**

**Requirement:** Qualify undelegation, maturity, liability settlement and owner withdrawals. Retain locked-principal restrictions and both evidence-age limits; release principal exactly once after all liabilities settle.

**Completed work**

- H+2 unbonding creates owner/validator-specific custody entries.
- Batch 9 locally checks exposure, dual maturity, penalties and one-time owner withdrawal.
- Batch 10 filters durable processed transactions before repeat admission; runtime checks restart and failed writes.
- Revalidated selected liability and withdrawal tests while preserving production guards. The current matrix separates development penalty-profile withdrawals from disabled production/lifecycle withdrawal and missing locked-principal liability qualification.

**Evidence**

- [batch-10/REPORT.md](batch-10/REPORT.md)
- [batch-10/TEST_RESULTS.json](batch-10/TEST_RESULTS.json)
- [batch-10/VALIDATION.json](batch-10/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [batch-9/REPORT.md](batch-9/REPORT.md)
- [batch-9/TEST_RESULTS.json](batch-9/TEST_RESULTS.json)
- [batch-9/VALIDATION.json](batch-9/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/behavior/RESULTS.json](decision-register/native-staging-production-closure/behavior/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve evidence limits, margins, penalty/withdrawal and locked-principal rules.
- Complete actual engine evidence coverage before enabling production withdrawals.
- Qualify maturity, rapid undelegation, failure, replay, key rotation and recovery on the frozen candidate.

**Acceptance requirement:** Qualify undelegation, maturity, liability settlement and owner withdrawals. Retain locked-principal restrictions and both evidence-age limits; release principal exactly once after all liabilities settle.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Validator/economics leads and independent liability reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production withdrawals remain disabled. Approved liability/evidence bounds, full fault attribution, locked-principal implementation, activation/migration and production withdrawal qualification remain open.

**Decision and record dependencies:** D08-Q01, D08-Q02, D08-Q03, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1151–1209](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json. The root-helper change has native local tests; the prior Linux application predates it. Native Hetzner qualification in this queue covers strict CLI startup only. No full native validator staging or production acceptance is claimed.

<a id="g19"></a>
## G19 — Rewards

**Status: PARTIAL**

**Requirement:** Distribute and claim rewards using approved finalized-state eligibility and integer allocation. Reconcile rewards, reserves, fees and supply; prevent duplicate claims after failure or restart.

**Completed work**

- Approved one-finalized-block intervals use parent-state eligibility, exact floor allocation and no staking-pool commission.
- Separate rounding/inactive reserves have no automatic recipient or sweep.
- Signed claim tests cover prewrite failure and lost acknowledgement; atomic issuance integration and actual-engine checks are retained.

**Evidence**

- [batch-6/APPROVAL.json](batch-6/APPROVAL.json)
- [batch-6/REPORT.md](batch-6/REPORT.md)
- [batch-6/TEST_RESULTS.json](batch-6/TEST_RESULTS.json)
- [batch-6/VALIDATION.json](batch-6/VALIDATION.json)
- [batch-6/claim-recovery/APPROVAL.json](batch-6/claim-recovery/APPROVAL.json)
- [batch-6/claim-recovery/REPORT.md](batch-6/claim-recovery/REPORT.md)
- [batch-6/claim-recovery/TEST_RESULTS.json](batch-6/claim-recovery/TEST_RESULTS.json)
- [batch-6/claim-recovery/VALIDATION.json](batch-6/claim-recovery/VALIDATION.json)
- [batch-6/integration-followup/APPROVAL.json](batch-6/integration-followup/APPROVAL.json)
- [batch-6/integration-followup/REPORT.md](batch-6/integration-followup/REPORT.md)
- [batch-6/integration-followup/TEST_RESULTS.json](batch-6/integration-followup/TEST_RESULTS.json)
- [batch-6/integration-followup/VALIDATION.json](batch-6/integration-followup/VALIDATION.json)
- [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json)
- [batch-6/issuance-timing/REPORT.md](batch-6/issuance-timing/REPORT.md)
- [batch-6/issuance-timing/TEST_RESULTS.json](batch-6/issuance-timing/TEST_RESULTS.json)
- [batch-6/issuance-timing/VALIDATION.json](batch-6/issuance-timing/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/ordinary-signing-implementation/APPROVAL.json](decision-register/ordinary-signing-implementation/APPROVAL.json)
- [decision-register/ordinary-signing-implementation/REPORT.md](decision-register/ordinary-signing-implementation/REPORT.md)
- [decision-register/ordinary-signing-implementation/TEST_RESULTS.json](decision-register/ordinary-signing-implementation/TEST_RESULTS.json)
- [decision-register/ordinary-signing-implementation/VALIDATION.json](decision-register/ordinary-signing-implementation/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve separate validator-pool eligibility, treasury recipient/custody, controller parameters and observation inputs.
- Qualify final reward recipients, interval edges, zero eligibility, claims, recovery and supply accounting.
- Retain the retired legacy rounding defect as diagnostic history; do not represent retirement as a repair.

**Acceptance requirement:** Distribute and claim rewards using approved finalized-state eligibility and integer allocation. Reconcile rewards, reserves, fees and supply; prevent duplicate claims after failure or restart.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Economics lead, treasury authority and independent accounting reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production validator-pool rules, treasury authority and issuance configuration remain unresolved.

**Decision and record dependencies:** D01-Q01, D01-Q02, D02-Q01, D02-Q02, D03-Q01, D11-Q01, D11-Q02, D11-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g20"></a>
## G20 — Fees

**Status: PARTIAL**

**Requirement:** Implement the approved ordinary and sponsored-recovery fee contracts. Qualify deterministic metering, funding reservations, accepted failures, out-of-gas, refunds, nonces and atomic fee custody without double charges.

**Completed work**

- RF01–RF06 and OF01–OF05 select uDRT fee custody, measured outcomes and zero-charge preacceptance rejection.
- Twelve ordinary actions and sponsored recovery share reservations and atomic settlement. Local runtime acceptance maps 20 requirements to 69 tests.
- Engine/browser tests verify charges, released caps, failures, out-of-gas and restart.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/ordinary-fee-implementation/APPROVAL.json](decision-register/ordinary-fee-implementation/APPROVAL.json)
- [decision-register/ordinary-fee-implementation/REPORT.md](decision-register/ordinary-fee-implementation/REPORT.md)
- [decision-register/ordinary-fee-implementation/TEST_RESULTS.json](decision-register/ordinary-fee-implementation/TEST_RESULTS.json)
- [decision-register/ordinary-fee-implementation/VALIDATION.json](decision-register/ordinary-fee-implementation/VALIDATION.json)
- [decision-register/ordinary-runtime-integration/ACCEPTANCE.json](decision-register/ordinary-runtime-integration/ACCEPTANCE.json)
- [decision-register/ordinary-runtime-integration/APPROVAL.json](decision-register/ordinary-runtime-integration/APPROVAL.json)
- [decision-register/ordinary-runtime-integration/REPORT.md](decision-register/ordinary-runtime-integration/REPORT.md)
- [decision-register/ordinary-runtime-integration/TEST_RESULTS.json](decision-register/ordinary-runtime-integration/TEST_RESULTS.json)
- [decision-register/ordinary-runtime-integration/VALIDATION.json](decision-register/ordinary-runtime-integration/VALIDATION.json)
- [decision-register/recovery-fee-implementation/ACCEPTANCE_STATUS.json](decision-register/recovery-fee-implementation/ACCEPTANCE_STATUS.json)
- [decision-register/recovery-fee-implementation/APPROVAL.json](decision-register/recovery-fee-implementation/APPROVAL.json)
- [decision-register/recovery-fee-implementation/REPORT.md](decision-register/recovery-fee-implementation/REPORT.md)
- [decision-register/recovery-fee-implementation/TEST_RESULTS.json](decision-register/recovery-fee-implementation/TEST_RESULTS.json)
- [decision-register/recovery-fee-implementation/VALIDATION.json](decision-register/recovery-fee-implementation/VALIDATION.json)
- [decision-register/recovery-fee-storage/ACCEPTANCE.json](decision-register/recovery-fee-storage/ACCEPTANCE.json)
- [decision-register/recovery-fee-storage/VALIDATION.json](decision-register/recovery-fee-storage/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve production fee rates, resource bounds, bootstrap funding and remaining burn/distribution authority.
- Calibrate limits on the frozen full network and qualify every fee outcome and recovery case.
- Obtain accounting and client acceptance tied to final configuration and artifacts.

**Acceptance requirement:** Implement the approved ordinary and sponsored-recovery fee contracts. Qualify deterministic metering, funding reservations, accepted failures, out-of-gas, refunds, nonces and atomic fee custody without double charges.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Runtime/economics leads and independent fee reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Production numeric fee/resource inputs and general burn/distribution policy remain open.

**Decision and record dependencies:** D04-Q01, D04-Q02, D05-Q01, D05-Q02, D08-Q01, D08-Q02, D08-Q03, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 803–857](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g21"></a>
## G21 — Slashing

**Status: PARTIAL**

**Requirement:** Define and qualify fault detection, evidence validation, jailing, penalties, repeat-fault treatment, reinstatement and penalty custody. Preserve historical liability and supply through key changes and withdrawals.

**Completed work**

- Batch 9 records historical principal exposure and schedules removal/penalty settlement at H+2.
- Twenty-two new local tests cover old keys, repeated evidence, custody, authorization, conservation and recovery.
- Revalidated existing bridge evidence and liability tests. Source review locates the coverage gap across engine attribution, bridge facts and application settlement; no fault policy or synthetic penalty was promoted to production.

**Evidence**

- [batch-10/REPORT.md](batch-10/REPORT.md)
- [batch-10/TEST_RESULTS.json](batch-10/TEST_RESULTS.json)
- [batch-10/VALIDATION.json](batch-10/VALIDATION.json)
- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [batch-9/REPORT.md](batch-9/REPORT.md)
- [batch-9/TEST_RESULTS.json](batch-9/TEST_RESULTS.json)
- [batch-9/VALIDATION.json](batch-9/VALIDATION.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/behavior/RESULTS.json](decision-register/native-staging-production-closure/behavior/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve fault types, rates, evidence ages, margins, repeat-fault/reinstatement and reserve rules.
- Complete cryptographically validated engine evidence coverage and locked-principal handling.
- Qualify real fault evidence and penalty settlement on the frozen candidate before withdrawal activation.

**Acceptance requirement:** Define and qualify fault detection, evidence validation, jailing, penalties, repeat-fault treatment, reinstatement and penalty custody. Preserve historical liability and supply through key changes and withdrawals.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Consensus/economics leads and independent evidence reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Existing duplicate-vote fixture coverage is bounded. Full engine evidence attribution, production penalty values, locked liability, activation/migration and end-to-end fault acceptance remain open.

**Decision and record dependencies:** D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D11-Q01, D11-Q02, D11-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1151–1209](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json. The root-helper change has native local tests; the prior Linux application predates it. Native Hetzner qualification in this queue covers strict CLI startup only. No full native validator staging or production acceptance is claimed.

<a id="g22"></a>
## G22 — Governance

**Status: PARTIAL**

**Requirement:** Qualify the full governance proposal, voting, tally, execution and recovery lifecycle. Enforce approved bonded-stake eligibility, common snapshots, thresholds, deposits, timelocks, parameter bounds and emergency authority.

**Completed work**

- Five original governance failures were resolved locally. Persisted vote enumeration, overflow checks and repeated-hook behavior have regression evidence.
- Bonded-stake-only voting is approved and implemented; liquid DGT gives zero power.
- Recorded the user-approved partial D11-Q03 emergency policy and implemented development freeze/resume control. Production authority membership, quorum, timing and automatic-transition policy remain unset.
- Recorded explicit approval of six additional D11-Q03 recommendations: three-of-five independent custodians, measured height-based timing, continued mandatory transitions, fresh resume evidence, candidate-specific upgrade clearance and separate full-halt recovery.
- Implemented the approved emergency policy structure with explicit development inputs. Freeze and resume use distinct three-of-five keys. Resume preserves the upgrade hold; activation needs separate exact-plan clearance.

**Evidence**

- [batch-5/BATCH_5_REPORT.md](batch-5/BATCH_5_REPORT.md)
- [batch-5/TEST_RESULTS.json](batch-5/TEST_RESULTS.json)
- [batch-5/VALIDATION.json](batch-5/VALIDATION.json)
- [batch-5/eligibility-followup/APPROVAL.json](batch-5/eligibility-followup/APPROVAL.json)
- [batch-5/eligibility-followup/REPORT.md](batch-5/eligibility-followup/REPORT.md)
- [batch-5/eligibility-followup/TEST_RESULTS.json](batch-5/eligibility-followup/TEST_RESULTS.json)
- [batch-5/eligibility-followup/VALIDATION.json](batch-5/eligibility-followup/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/security/REPORT.md](decision-register/launch-preparation-workstreams/security/REPORT.md)
- [decision-register/launch-preparation-workstreams/security/RESULTS.json](decision-register/launch-preparation-workstreams/security/RESULTS.json)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/policy/APPROVAL.json](decision-register/emergency-transaction-freeze/policy/APPROVAL.json)
- [decision-register/emergency-transaction-freeze/policy/DECISION_AMENDMENT.json](decision-register/emergency-transaction-freeze/policy/DECISION_AMENDMENT.json)
- [decision-register/emergency-transaction-freeze/policy/POLICY_CONTRACT.md](decision-register/emergency-transaction-freeze/policy/POLICY_CONTRACT.md)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/policy/APPROVAL.json](decision-register/emergency-release-staging/policy/APPROVAL.json)
- [decision-register/emergency-release-staging/policy/DECISION_AMENDMENT.json](decision-register/emergency-release-staging/policy/DECISION_AMENDMENT.json)
- [decision-register/emergency-release-staging/policy/CURRENT_APPROVAL_STATUS.md](decision-register/emergency-release-staging/policy/CURRENT_APPROVAL_STATUS.md)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/emergency/RESULTS.json](decision-register/emergency-upgrade-execution/emergency/RESULTS.json)
- [decision-register/emergency-upgrade-execution/upgrade/RESULTS.json](decision-register/emergency-upgrade-execution/upgrade/RESULTS.json)
- [decision-register/emergency-upgrade-execution/custody/REPORT.md](decision-register/emergency-upgrade-execution/custody/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve snapshot timing, vote ownership, validator eligibility, quorum/veto/abstention, deposits, periods and timelocks.
- Complete configuration restore, deposit conservation, crash-atomic execution and authorized parameter changes.
- Qualify governance concentration, abuse rejection and recovery on the final economic network.

**Acceptance requirement:** Qualify the full governance proposal, voting, tally, execution and recovery lifecycle. Enforce approved bonded-stake eligibility, common snapshots, thresholds, deposits, timelocks, parameter bounds and emergency authority.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L04: Explicit beneficiary schedules, base-unit totals, approved parameters and deterministic input/output comparison.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** Governance/economics leads and independent governance reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Custodian identities, keys, epochs, measured numeric limits, production wire acceptance and upgrade authority inputs remain open. Broader governance snapshot, timing and execution decisions remain open.

**Decision and record dependencies:** D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D11-Q01, D11-Q02, D11-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 273–341](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1033–1093](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1151–1209](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g23"></a>
## G23 — Treasury controls

**Status: PARTIAL**

**Requirement:** Define and enforce treasury receipt, custody and spending authority. Qualify authorized and rejected actions, conservation, audit records, replay protection and recovery.

**Completed work**

- Issuance integration places treasury allocation in explicit custody without granting recipient or sweep authority.
- Treasury record packet and custody procedures identify required fields and acceptance evidence.
- Maintained root module now has checked signing and an action/sequence coordinator; production consumers remain absent.

**Evidence**

- [batch-6/issuance-timing/APPROVAL.json](batch-6/issuance-timing/APPROVAL.json)
- [batch-6/issuance-timing/REPORT.md](batch-6/issuance-timing/REPORT.md)
- [batch-6/issuance-timing/TEST_RESULTS.json](batch-6/issuance-timing/TEST_RESULTS.json)
- [batch-6/issuance-timing/VALIDATION.json](batch-6/issuance-timing/VALIDATION.json)
- [decision-register/gate-closure-implementation/root/REPORT.md](decision-register/gate-closure-implementation/root/REPORT.md)
- [decision-register/gate-closure-implementation/root/RESULTS.json](decision-register/gate-closure-implementation/root/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-2/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-2/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-2/REPORT.md](decision-register/parallel-tracks-20260912/track-2/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-2/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-2/TEST_RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Verify treasury recipients and custodians; approve governance, spending and emergency authorities.
- Integrate actual treasury authorization and durable action execution.
- Qualify permitted/rejected spending, replay, failure and recovery with independent accounting review.

**Acceptance requirement:** Define and enforce treasury receipt, custody and spending authority. Qualify authorized and rejected actions, conservation, audit records, replay protection and recovery.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L01: Verified preparable fields; approved input policy and role assignments. Final candidate, production-key and genesis bindings remain pending until L11-L13.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.
- L10: Explicit ceremony authorization, public key validation, custody roles and recovery procedures. Never export private material.

**Acceptance authority:** Treasury authority, custodians and independent control reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Treasury recipient/custody record D02-Q02 and governance authority D11-Q03 remain open; custody balances do not authorize spending.

**Decision and record dependencies:** D02-Q01, D02-Q02, D05-Q01, D05-Q02, D10-Q01, D10-Q02, D10-Q03, D11-Q01, D11-Q02, D11-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 419–533](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1151–1209](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Use saved package source and executable hashes for credited results. The active implementation tree has later changes; SOURCE_DRIFT.json records the inspected differences. Changed source needs new qualification.

<a id="g24"></a>
## G24 — Upgrade procedure

**Status: PARTIAL**

**Requirement:** Qualify a controlled RC1-to-RC2 upgrade with reviewed state migration and recovery. Test late/missing validators, old RPC nodes, wallet activity, rollback compatibility and the post-upgrade economic lifecycle.

**Completed work**

- Same-version executable replacement and original-path restoration preserve receipts and continued blocks.
- Release tooling defines a transition contract and rejects seven invalid transition cases, including migration.
- Maintained root action coordinator prepares an authorization boundary but has no actual upgrade consumer.
- The selected application storage features were compared with the legacy build. LZ 4-only fresh genesis/restart passed; loss of inherited Snappy support is now recorded explicitly.
- Restored explicit LZ4+Snappy features without legacy core/PQC implementations. Two actual compressed-SST tests passed with WAL disabled, two Storage::open reopens,256 intact records and unchanged SST bytes; all 30 existing selected storage tests also passed. New native consensus restart passed.
- Passed two actual Linux LZ4/Snappy compressed-SST reopen tests and 30 storage library tests using the exact fresh application-build native archives. Added a development pending-upgrade intent recorder. No upgrade or halt executes.
- Mapped future root action state, immutable receipts and replay sequences to the actual synchronous chain commit and recovery paths. Prepared concrete priority/cancellation/resume proposals; no operational consumer or production upgrade was enabled.
- Implemented a persistent upgrade hold set by freeze. Resume does not clear that hold. No production upgrade activation consumer exists; upgrade execution and hold-release qualification remain open.
- Inspected actual upgrade paths and confirmed that no binary or state-migration executor exists. Prepared a chain-atomic executor design, freeze/hold guard location and sixteen proposed acceptance cases. No synthetic version-only activation was added.
- Implemented signed admission, activation and cancellation with a real nonempty emergency-receipt index migration. State and migration writes share the consensus database batch. Actual signatures, query use, replay, failure and restart checks passed.

**Evidence**

- [decision-register/gate-closure-implementation/root/REPORT.md](decision-register/gate-closure-implementation/root/REPORT.md)
- [decision-register/gate-closure-implementation/root/RESULTS.json](decision-register/gate-closure-implementation/root/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json](decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json)
- [decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json](decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json)
- [decision-register/launch-preparation-workstreams/release/REPORT.md](decision-register/launch-preparation-workstreams/release/REPORT.md)
- [decision-register/launch-preparation-workstreams/release/RESULTS.json](decision-register/launch-preparation-workstreams/release/RESULTS.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/rust/REPORT.md](decision-register/production-boundary-qualification/rust/REPORT.md)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/storage/RESULTS.json](decision-register/production-profile-finalization/storage/RESULTS.json)
- [decision-register/production-profile-finalization/storage/fresh-native-archives.json](decision-register/production-profile-finalization/storage/fresh-native-archives.json)
- [decision-register/production-profile-finalization/integration/native01/RUNTIME.json](decision-register/production-profile-finalization/integration/native01/RUNTIME.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/linux/REPORT.md](decision-register/linux-release-authority-qualification/linux/REPORT.md)
- [decision-register/linux-release-authority-qualification/linux/RESULTS.json](decision-register/linux-release-authority-qualification/linux/RESULTS.json)
- [decision-register/linux-release-authority-qualification/root/REPORT.md](decision-register/linux-release-authority-qualification/root/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md](decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md)
- [decision-register/native-staging-production-closure/root/RESULTS.json](decision-register/native-staging-production-closure/root/RESULTS.json)
- [decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md](decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/state/REPORT.md](decision-register/emergency-transaction-freeze/state/REPORT.md)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-transaction-freeze/review/REVIEW.md](decision-register/emergency-transaction-freeze/review/REVIEW.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/upgrade/EXECUTOR_DESIGN.md](decision-register/emergency-release-staging/upgrade/EXECUTOR_DESIGN.md)
- [decision-register/emergency-release-staging/upgrade/ACCEPTANCE_MATRIX.md](decision-register/emergency-release-staging/upgrade/ACCEPTANCE_MATRIX.md)
- [decision-register/emergency-release-staging/upgrade/RESULTS.json](decision-register/emergency-release-staging/upgrade/RESULTS.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/upgrade/REPORT.md](decision-register/emergency-upgrade-execution/upgrade/REPORT.md)
- [decision-register/emergency-upgrade-execution/consensus/REPORT.md](decision-register/emergency-upgrade-execution/consensus/REPORT.md)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)
- [decision-register/emergency-upgrade-execution/review/REVIEW.md](decision-register/emergency-upgrade-execution/review/REVIEW.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve the production storage/compression profile and qualify complete existing state on the frozen Linux release. Prior compressed-SST fixture reads remain scoped evidence.
- Accept production control schemas, authority membership/thresholds and action priority/cancellation. Preserve exact migration versions and qualify cross-binary authorization, state reopening and execution.
- Run the complete cross-version migration/rollback matrix with late or missing validators, old RPC nodes, wallet activity and post-upgrade economics; obtain independent review and complete the applicable final simulation.

**Acceptance requirement:** Qualify a controlled RC1-to-RC2 upgrade with reviewed state migration and recovery. Test late/missing validators, old RPC nodes, wallet activity, rollback compatibility and the post-upgrade economic lifecycle.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L06: Signer exclusivity, backup epoch, anti-double-sign state, state compatibility and reviewed source/target upgrade pair.

**Acceptance authority:** Release lead, operators and independent migration reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** The first executor works for the root-bound running candidate. Version-preserving migration history, cross-binary installation/reopen/rollback, production authority inputs and full RC1-to-RC2 qualification remain open.

**Decision and record dependencies:** D10-Q01, D10-Q02, D10-Q03, D11-Q01, D11-Q02, D11-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 1211–1295](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1515–1553](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g25"></a>
## G25 — RPC stability

**Status: PARTIAL**

**Requirement:** Qualify stable production RPC service and correct committed-state interfaces under load, interruption, reconnect, restart and upgrade. Verify endpoint trust, receipts and applicable client proofs.

**Completed work**

- Native and browser modules exercise actual engine RPC with matching receipt fields on four endpoints and restart persistence.
- The earlier TCP RPC compatibility run passed 154 assertions and six transport checks.
- SDK queries distinguish committed state from provisional admission.
- Implemented bounded private Unix RPC dispatch and a separate Hyper HTTP/1 adapter. Four actual engines passed 154 consensus/RPC assertions plus six transport checks. Default and prior-profile tests passed. Unsupported WebSocket and other omitted interface routes remain explicit.
- The Linux service run exercised the native CLI through the actual engine TCP RPC. The byte-forwarding test adapter did not synthesize chain results. Thirty-six service checks passed; the newer IPC/Hyper Linux service profile remains untested.
- The selected IPC engine, tagged bridge, Hyper adapter and HTTP-only ordinary client passed combined native and x86_64 process runs. The supervisor uses explicit builtin hashes without OpenSSL hash fallback.
- Exercised the selected Linux IPC/HTTP path as installed systemd services with the packaged Linux CLI. Verified committed receipts and restart behavior. No public production endpoint, hosted wallet or capacity target was qualified.
- Reviewed required proof/snapshot and interface dispositions. Explicit unavailable interfaces remain recorded; native CLI startup does not qualify public RPC or committed-state proofs.
- The pinned engine, bridge, application, adapter and strict client completed native private-network submission, commitment, receipt, control and restart checks.
- Added emergency receipt lookup by digest through the process interface. Queries return chain, genesis, height and application hash. Local tests verify index creation, lookup, later maintenance and changed-history refusal.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/ordinary-client-compatibility/ACCEPTANCE.json](decision-register/ordinary-client-compatibility/ACCEPTANCE.json)
- [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json)
- [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md)
- [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json)
- [decision-register/ordinary-client-compatibility/VALIDATION.json](decision-register/ordinary-client-compatibility/VALIDATION.json)
- [decision-register/pqc-build-boundary/ACCEPTANCE.json](decision-register/pqc-build-boundary/ACCEPTANCE.json)
- [decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json](decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-build-boundary/REPORT.md](decision-register/pqc-build-boundary/REPORT.md)
- [decision-register/pqc-build-boundary/TEST_RESULTS.json](decision-register/pqc-build-boundary/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/VALIDATION.json](decision-register/pqc-build-boundary/VALIDATION.json)
- [decision-register/production-integration/pqc/QUALIFICATION.json](decision-register/production-integration/pqc/QUALIFICATION.json)
- [decision-register/production-integration/pqc/PQC_IMPLEMENTATION_REPORT.md](decision-register/production-integration/pqc/PQC_IMPLEMENTATION_REPORT.md)
- [decision-register/production-integration/pqc/http-adapter/RESULTS.json](decision-register/production-integration/pqc/http-adapter/RESULTS.json)
- [decision-register/production-integration/pqc/ipc-runtime/RESULTS.json](decision-register/production-integration/pqc/ipc-runtime/RESULTS.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json](decision-register/production-integration/linux/retry/evidence/LINUX_SYSTEMD_QUALIFICATION.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/RESULTS.json](decision-register/linux-release-authority-qualification/services/RESULTS.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md](decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/consensus/REPORT.md](decision-register/emergency-upgrade-execution/consensus/REPORT.md)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Freeze the permitted RPC/interface and transport boundary under G35.
- Run full Linux load, outage/reconnect/upgrade and retention checks against approved service targets.
- Complete independent endpoint/state and receipt inclusion verification.

**Acceptance requirement:** Qualify stable production RPC service and correct committed-state interfaces under load, interruption, reconnect, restart and upgrade. Verify endpoint trust, receipts and applicable client proofs.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.
- L08: Controlled staging measurements; approved limits and recovery objectives; no conversion of component rates into TPS.

**Acceptance authority:** RPC/SRE leads and independent client reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Changed Rust code has local executable evidence. Current Linux and actual-engine qualification must be repeated. Hosted endpoint trust, receipt/state proofs, public transport, production services and load/outage/upgrade targets remain open.

**Decision and record dependencies:** D06-Q01, D06-Q02, D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 273–341](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 803–951](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1095–1149](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g26"></a>
## G26 — Load capacity

**Status: PARTIAL**

**Requirement:** Measure safe sustained and peak committed transaction capacity, block utilization, propagation, mempool behavior, RPC latency and resource use on the frozen production-equivalent network. Set limits from observed saturation and recovery.

**Completed work**

- Batch 4 retained separate debug and optimized admission measurements without changing timing limits.
- Performance preparation measured seven components in 35 batches; all 3,500 operations passed correctness checks. Ten result-validator tests passed.

**Evidence**

- [batch-4/BATCH_4_REPORT.md](batch-4/BATCH_4_REPORT.md)
- [batch-4/TEST_RESULTS.json](batch-4/TEST_RESULTS.json)
- [batch-4/VALIDATION.json](batch-4/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/performance/REPORT.md](decision-register/launch-preparation-workstreams/performance/REPORT.md)
- [decision-register/launch-preparation-workstreams/performance/RESULTS.json](decision-register/launch-preparation-workstreams/performance/RESULTS.json)
- [decision-register/launch-preparation-workstreams/performance/TEST_RESULTS.json](decision-register/launch-preparation-workstreams/performance/TEST_RESULTS.json)
- [decision-register/launch-preparation-workstreams/performance/VALIDATION.json](decision-register/launch-preparation-workstreams/performance/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve workload, latency, capacity and recovery thresholds and intended hardware.
- Measure authenticated wire transport and committed transactions on controlled Linux staging.
- Capture latency distributions, throughput, queue, CPU, memory, disk/network use and recovery; repeat and obtain acceptance.

**Acceptance requirement:** Measure safe sustained and peak committed transaction capacity, block utilization, propagation, mempool behavior, RPC latency and resource use on the frozen production-equivalent network. Set limits from observed saturation and recovery.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L08: Controlled staging measurements; approved limits and recovery objectives; no conversion of component rates into TPS.

**Acceptance authority:** Performance lead and independent capacity reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Component timings and queue benchmarks do not measure sustained committed network throughput. Production targets remain unset.

**Decision and record dependencies:** D06-Q01, D06-Q02, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 1095–1149](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1593–1649](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g27"></a>
## G27 — Monitoring

**Status: PARTIAL**

**Requirement:** Deploy monitoring for every required consensus, validator, transaction, accounting, governance, resource and RPC measurement. Verify accurate collection, retention, dashboards and independent monitoring-service health.

**Completed work**

- Infrastructure review and draft runbook identify required monitoring controls.
- Runbook tooling checks fifteen ordered steps and all 23 prerequisite edges.
- Local engine and operations evidence records height, receipts, state and restart observations.
- Mapped the disabled embedded metrics listener to the still-required monitoring deliverable. Omitting the listener does not waive production telemetry coverage or select a replacement interface.

**Evidence**

- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md](decision-register/native-staging-production-closure/decisions/DECISION_REQUESTS.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Implement the full production measurement/export path compatible with G35; do not infer coverage from local logs.
- Approve thresholds, retention, service targets and named owners.
- Verify all listed Day 9 measurements and independent monitoring-service health on the frozen topology.

**Acceptance requirement:** Deploy monitoring for every required consensus, validator, transaction, accounting, governance, resource and RPC measurement. Verify accurate collection, retention, dashboards and independent monitoring-service health.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.
- L08: Controlled staging measurements; approved limits and recovery objectives; no conversion of component rates into TPS.

**Acceptance authority:** SRE lead and independent operations reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** The selected build omits the embedded metrics listener. Required monitoring interface/coverage, accepted owner, thresholds and operational validation remain open.

**Decision and record dependencies:** D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 859–951](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json. The root-helper change has native local tests; the prior Linux application predates it. Native Hetzner qualification in this queue covers strict CLI startup only. No full native validator staging or production acceptance is claimed.

<a id="g28"></a>
## G28 — Alerting

**Status: PARTIAL**

**Requirement:** Deliver actionable alerts for every required halt, outage, supply, staking, governance, signature-failure and restart-loop condition. Verify receipt, acknowledgement, escalation and monitoring-service failure detection.

**Completed work**

- Runbook preparation defines alert verification and communication fields.
- Historical infrastructure and service preflight evidence is retained; no live alert acceptance is claimed.

**Evidence**

- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Configure all Day 9 alert conditions, thresholds, authenticated routing and named primary/backup contacts.
- Exercise each alert and record actual delivery, acknowledgement and escalation.
- Verify monitoring failure detection and approve on-call coverage against service objectives.

**Acceptance requirement:** Deliver actionable alerts for every required halt, outage, supply, staking, governance, signature-failure and restart-loop condition. Verify receipt, acknowledgement, escalation and monitoring-service failure detection.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.
- L08: Controlled staging measurements; approved limits and recovery objectives; no conversion of component rates into TPS.

**Acceptance authority:** SRE/on-call lead and independent operations reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Alert thresholds, contacts and operational acceptance remain unset. Checklist validation does not demonstrate alert delivery.

**Decision and record dependencies:** D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 859–951](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g29"></a>
## G29 — Backup procedures

**Status: PARTIAL**

**Requirement:** Create documented production snapshot and backup procedures. Verify consistent application/consensus/signer backups, retention, protected off-host storage and successful restoration against approved recovery objectives.

**Completed work**

- Stopped local backup restored 79 files and retained exact state and receipts across four validators.
- Recovery and release contracts require backup verification and signing-state continuity before replacement.

**Evidence**

- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json](decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json)
- [decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json](decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json)
- [decision-register/launch-preparation-workstreams/release/REPORT.md](decision-register/launch-preparation-workstreams/release/REPORT.md)
- [decision-register/launch-preparation-workstreams/release/RESULTS.json](decision-register/launch-preparation-workstreams/release/RESULTS.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve backup frequency, retention, custody, off-host destination and recovery objectives.
- Qualify consistent backups and off-host restore of the complete frozen Linux stack.
- Demonstrate stale-signing-state rejection, signer exclusivity and operator execution of the written procedure.

**Acceptance requirement:** Create documented production snapshot and backup procedures. Verify consistent application/consensus/signer backups, retention, protected off-host storage and successful restoration against approved recovery objectives.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.

**Acceptance authority:** Storage/SRE leads, custodians and independent recovery reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Same-host stopped-fleet backup does not qualify off-host storage, stale-signing safety or production backup objectives.

**Decision and record dependencies:** D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 953–1025](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g30"></a>
## G30 — Disaster recovery

**Status: PARTIAL**

**Requirement:** Recover from the required server, state, disk, network and resource failures using documented procedures. Preserve finalized history, supply and exclusive signing; meet approved service and recovery objectives.

**Completed work**

- Five local four-validator drills cover graceful restart, abrupt process loss, stopped restore and same-version replacement/restoration.
- All 261 checks passed and block commitment resumed. Runbook preparation defines halt and recovery conditions.

**Evidence**

- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/operations/REPORT.md](decision-register/launch-preparation-workstreams/operations/REPORT.md)
- [decision-register/launch-preparation-workstreams/operations/RESULTS.json](decision-register/launch-preparation-workstreams/operations/RESULTS.json)
- [decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json](decision-register/launch-preparation-workstreams/operations/evidence/ENGINE_OPERATIONS.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Qualify the remaining corruption, server/disk loss, partition, resource-pressure and off-host scenarios on approved disposable Linux staging.
- Measure each approved recovery objective and reconcile state, supply, stake and signing safety.
- Obtain named operator and independent recovery acceptance.

**Acceptance requirement:** Recover from the required server, state, disk, network and resource failures using documented procedures. Preserve finalized history, supply and exclusive signing; meet approved service and recovery objectives.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L06: Signer exclusivity, backup epoch, anti-double-sign state, state compatibility and reviewed source/target upgrade pair.

**Acceptance authority:** SRE/incident lead and independent recovery reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** The full failure matrix and off-host Linux recovery remain unexecuted; five local drills cover only a subset.

**Decision and record dependencies:** D06-Q01, D06-Q02, D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 953–1025](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1431–1513](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

<a id="g31"></a>
## G31 — Security

**Status: PARTIAL**

**Requirement:** Complete independent security review of the frozen protocol, cryptography, economic controls, interfaces, keys, dependencies, build/distribution chain and server configuration. Resolve every P0 and absolute launch blocker.

**Completed work**

- Security preparation defines 14 open review questions across nine areas, with 28 source/evidence hashes and 18 result/log hashes. Fifteen validator tests passed.
- Source reviews and malformed-input, replay, authority and atomicity checks exist across implementation packages.
- Maintained root integration and narrowed engine build add evidence while retaining explicit G35 and storage limitations.
- Completed internal source reviews of the durable root file adapter, actual Rust genesis consumer/helper, and selected Go provider files. Exact source pins match. The review handoff records unresolved production boundaries and the current Rust P-256 artifact evidence.
- Recorded Rick Glenn, CTO at Dytallix, as the supplied nominee for release coordination, validator operations, custody and review. His affiliation does not establish independent security review.
- Inspected the exact Linux ELF and the resolved providers in both immutable build and service images. Hashed libraries contain defined RSA, ECDSA, ECDH and TLS implementation symbols. This confirms provider presence without claiming application invocation.
- Internal reviews corrected two Cargo feature guard gaps and runtime cleanup defects. Final source/runtime reviews, full negative compiler checks and hashed component inventories are retained.
- Internal review checked feature separation, legacy compatibility, codec tests and root evidence validation. Bounded-read and stale-status issues were corrected; 23 root record-validator tests passed. Independent security acceptance was not granted.
- Completed internal provider classification for three exact static Linux Go executables and internal review of the development root intent component and service evidence. Recorded unavailable emulator mapping and finite inspection limits. No independent security signoff was issued.
- Completed bounded internal source/evidence review of helper changes and actual root persistence boundaries. Verified host isolation and cleanup for native CLI startup; preserved full OS/provider and independent acceptance limits.
- Added local correctness review and bounded emergency authority tests. Recorded separate operational halt preparation and unresolved production authority and qualification requirements. Internal agent review is not independent acceptance.
- Preserved pinned build and approval evidence, verified the exact two-amendment approval chain with 24 tests, and prepared bounded shared-host preservation and isolation controls. Internal review does not supply independent acceptance.
- Completed an internal review of 13 maintained source files. Resolved anchor-test isolation, same-block priority and receipt-query context. No blocking implementation finding remains within this scope.
- Candidate 15 deadline evidence isolated timing variance and an EACCES result for external SIGSTOP. The policy does not grant unconfined STOP to the helper. This was an internal development probe, not security acceptance.

**Evidence**

- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/gate-closure-implementation/root/REPORT.md](decision-register/gate-closure-implementation/root/REPORT.md)
- [decision-register/gate-closure-implementation/root/RESULTS.json](decision-register/gate-closure-implementation/root/RESULTS.json)
- [decision-register/launch-preparation-workstreams/security/REPORT.md](decision-register/launch-preparation-workstreams/security/REPORT.md)
- [decision-register/launch-preparation-workstreams/security/RESULTS.json](decision-register/launch-preparation-workstreams/security/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-2/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-2/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-2/REPORT.md](decision-register/parallel-tracks-20260912/track-2/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-2/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-2/TEST_RESULTS.json)
- [decision-register/production-integration/root/INTERNAL_REVIEW.md](decision-register/production-integration/root/INTERNAL_REVIEW.md)
- [decision-register/production-integration/root/INTERNAL_RUST_REVIEW.json](decision-register/production-integration/root/INTERNAL_RUST_REVIEW.json)
- [decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json](decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json)
- [decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json](decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json)
- [decision-register/production-integration/REVIEW_HANDOFF.md](decision-register/production-integration/REVIEW_HANDOFF.md)
- [decision-register/production-integration/records/ROLE_NOMINATIONS.json](decision-register/production-integration/records/ROLE_NOMINATIONS.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json](decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_LINKED_PROVIDERS.json](decision-register/production-integration/linux/retry/evidence/LINUX_LINKED_PROVIDERS.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/review/RUST_PROFILE_REVIEW.md](decision-register/production-boundary-qualification/review/RUST_PROFILE_REVIEW.md)
- [decision-register/production-boundary-qualification/review/SDK_SUPERVISOR_REVIEW.md](decision-register/production-boundary-qualification/review/SDK_SUPERVISOR_REVIEW.md)
- [decision-register/production-boundary-qualification/review/LINUX_RUNNER_REVIEW.md](decision-register/production-boundary-qualification/review/LINUX_RUNNER_REVIEW.md)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/review/INTERNAL_CORRECTNESS_REVIEW.json](decision-register/production-profile-finalization/review/INTERNAL_CORRECTNESS_REVIEW.json)
- [decision-register/production-profile-finalization/review/REVIEW_RESOLUTIONS.json](decision-register/production-profile-finalization/review/REVIEW_RESOLUTIONS.json)
- [decision-register/production-profile-finalization/records/RESULTS.json](decision-register/production-profile-finalization/records/RESULTS.json)
- [decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md](decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/providers/REPORT.md](decision-register/linux-release-authority-qualification/providers/REPORT.md)
- [decision-register/linux-release-authority-qualification/review/DEVELOPMENT_INTENT_INTERNAL_REVIEW.json](decision-register/linux-release-authority-qualification/review/DEVELOPMENT_INTENT_INTERNAL_REVIEW.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/review/SYSTEMD_HARNESS_INTERNAL_REVIEW.json](decision-register/linux-release-authority-qualification/review/SYSTEMD_HARNESS_INTERNAL_REVIEW.json)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/review/ROOT_HELPER_REVIEW.json](decision-register/native-staging-production-closure/review/ROOT_HELPER_REVIEW.json)
- [decision-register/native-staging-production-closure/root/BOUNDARY_REVIEW.json](decision-register/native-staging-production-closure/root/BOUNDARY_REVIEW.json)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/review/REVIEW.md](decision-register/emergency-transaction-freeze/review/REVIEW.md)
- [decision-register/emergency-transaction-freeze/policy/POLICY_CONTRACT.md](decision-register/emergency-transaction-freeze/policy/POLICY_CONTRACT.md)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/policy/VALIDATOR_IMPLEMENTATION.json](decision-register/emergency-release-staging/policy/VALIDATOR_IMPLEMENTATION.json)
- [decision-register/emergency-release-staging/linux/REPORT.md](decision-register/emergency-release-staging/linux/REPORT.md)
- [decision-register/emergency-release-staging/staging/REPORT.md](decision-register/emergency-release-staging/staging/REPORT.md)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/review/VALIDATION.json](decision-register/emergency-upgrade-execution/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/review/REVIEW.md](decision-register/emergency-upgrade-execution/review/REVIEW.md)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-timing-diagnostic-v1/DIAGNOSTIC_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-deadline-timing-diagnostic-v1/DIAGNOSTIC_RECORD.json)
- [decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-deadline-stall-native-v1/FAILURE_RECORD.json](decision-register/cross-binary-release/native-launch-owner/execution-policy/production-owner-integration-v1/helper-candidate15-supervisor-deadline-stall-native-v1/FAILURE_RECORD.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Assign independent reviewers and approved review scopes tied to exact candidate hashes.
- Resolve dossier questions, full dependency/provider and effective storage-path review; qualify production root consumers, external artifact verification and custody.
- Close all P0/absolute blockers and repeat affected checks after candidate changes.
- Review the post-GO helper cutoff and typed deadline race; reject test methods that widen production signal permissions. Independently review any corrected candidate and repeat affected native tests.

**Acceptance requirement:** Complete independent security review of the frozen protocol, cryptography, economic controls, interfaces, keys, dependencies, build/distribution chain and server configuration. Resolve every P0 and absolute launch blocker.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L03: Review scope, frozen candidate hashes, findings, remediation checks and signed acceptance.

**Acceptance authority:** Independent security auditors and accountable release authority. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Independent security acceptance of the frozen candidate, production authority/custody, full distribution/provider review and complete production behavior qualification remain open.

**Decision and record dependencies:** D07-Q01, D10-Q01, D10-Q02, D10-Q03, D11-Q01, D11-Q02, D11-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 1211–1295](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1857–1909](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g32"></a>
## G32 — Documentation

**Status: PARTIAL**

**Requirement:** Complete and freeze the required specifications, build/deployment instructions, operational and recovery runbooks, test reports and release evidence. Verify that independent engineers and operators can use them without undocumented knowledge.

**Completed work**

- Protocol, economics, genesis and infrastructure drafts retain approval and gap records.
- Fifteen-step runbook maps all 35 gates and eight production records; fifteen structural tests pass.
- Maintained service and repeatable wallet-build instructions add executable local procedures.
- Added source-bound implementation reports, explicit input-binding limits, a review handoff and the supplied role nominations. Preserved earlier reports and their hashes. Current consumer reconciliation distinguishes the implemented development root path from unsupported production adapters.
- The maintained Linux build guide now records the actual build, reduced-profile failure, 36-check service result, cleanup and remaining architecture/provider limits. Earlier failed build and harness records remain unchanged.
- Maintained profile/service/client procedures, source checks and the CI signature-policy target were updated. A seven-question production input packet uses the existing canonical IDs.
- Updated maintained Linux build and service documentation for the selected component set, bounded systemd test setup and verified-helper scratch execution requirement. Prepared field-level evidence for 112 populated production inputs while preserving unset production values.
- Added helper execution documentation and source-bound root/validator implementation contracts. Reconciled all34 existing questions with required work and acceptance evidence in one supplemental decision packet.
- Documented the development emergency control contract and prepared a separate full-consensus-halt procedure. Corrected the stale engineering percentage explanation; no measured replacement score is claimed.
- Documented emergency service configuration, real upgrade executor requirements and current production-input dependencies. Corrected stale maintained root documentation and linked explicit user approvals.
- Updated maintained emergency and upgrade specifications, source snapshots, current work queue and public custody intake. Recorded test scope and explicit cross-binary version-preservation limits.

**Evidence**

- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/launch-preparation-workstreams/genesis/REPORT.md](decision-register/launch-preparation-workstreams/genesis/REPORT.md)
- [decision-register/launch-preparation-workstreams/genesis/RESULTS.json](decision-register/launch-preparation-workstreams/genesis/RESULTS.json)
- [decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json](decision-register/launch-preparation-workstreams/release/EVIDENCE_DIGESTS.json)
- [decision-register/launch-preparation-workstreams/release/REPORT.md](decision-register/launch-preparation-workstreams/release/REPORT.md)
- [decision-register/launch-preparation-workstreams/release/RESULTS.json](decision-register/launch-preparation-workstreams/release/RESULTS.json)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/production-integration/REVIEW_HANDOFF.md](decision-register/production-integration/REVIEW_HANDOFF.md)
- [decision-register/production-integration/records/REPORT.md](decision-register/production-integration/records/REPORT.md)
- [decision-register/production-integration/records/ROLE_NOMINATIONS.md](decision-register/production-integration/records/ROLE_NOMINATIONS.md)
- [decision-register/production-integration/records/bindings/CONSUMER_RECONCILIATION/ASSESSMENT.md](decision-register/production-integration/records/bindings/CONSUMER_RECONCILIATION/ASSESSMENT.md)
- [decision-register/production-integration/linux/retry/REPORT.md](decision-register/production-integration/linux/retry/REPORT.md)
- [decision-register/production-integration/linux/retry/SERVICE_SOURCE_AFTER.json](decision-register/production-integration/linux/retry/SERVICE_SOURCE_AFTER.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/rust/RESULTS.json](decision-register/production-boundary-qualification/rust/RESULTS.json)
- [decision-register/production-boundary-qualification/integration/REPORT.md](decision-register/production-boundary-qualification/integration/REPORT.md)
- [decision-register/production-boundary-qualification/records/DECISION_QUEUE.md](decision-register/production-boundary-qualification/records/DECISION_QUEUE.md)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/documentation/CHANGES.json](decision-register/linux-release-authority-qualification/documentation/CHANGES.json)
- [decision-register/linux-release-authority-qualification/records/REPORT.md](decision-register/linux-release-authority-qualification/records/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/root-helper/REPORT.md](decision-register/native-staging-production-closure/root-helper/REPORT.md)
- [decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md](decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md)
- [decision-register/native-staging-production-closure/decisions/REPORT.md](decision-register/native-staging-production-closure/decisions/REPORT.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/evidence/parent-sources/emergency-transaction-freeze.md](decision-register/emergency-transaction-freeze/evidence/parent-sources/emergency-transaction-freeze.md)
- [decision-register/emergency-transaction-freeze/evidence/parent-sources/full-consensus-halt-procedure.draft.md](decision-register/emergency-transaction-freeze/evidence/parent-sources/full-consensus-halt-procedure.draft.md)
- [decision-register/emergency-transaction-freeze/policy/PROGRESS_ASSESSMENT.md](decision-register/emergency-transaction-freeze/policy/PROGRESS_ASSESSMENT.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/service/REPORT.md](decision-register/emergency-release-staging/service/REPORT.md)
- [decision-register/emergency-release-staging/upgrade/DOCUMENT_CHANGES.json](decision-register/emergency-release-staging/upgrade/DOCUMENT_CHANGES.json)
- [decision-register/emergency-release-staging/records/REPORT.md](decision-register/emergency-release-staging/records/REPORT.md)
- [decision-register/emergency-release-staging/policy/CURRENT_APPROVAL_STATUS.md](decision-register/emergency-release-staging/policy/CURRENT_APPROVAL_STATUS.md)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/custody/INTAKE.md](decision-register/emergency-upgrade-execution/custody/INTAKE.md)
- [decision-register/emergency-upgrade-execution/review/REVIEW.md](decision-register/emergency-upgrade-execution/review/REVIEW.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Complete the required artifact set and replace unset commands, thresholds, recovery objectives and named roles with accepted values.
- Update documents to the final maintained candidate and resolve all contradictory historical statements.
- Obtain two independent specification interpretations and operator execution/review of the final runbooks.

**Acceptance requirement:** Complete and freeze the required specifications, build/deployment instructions, operational and recovery runbooks, test reports and release evidence. Verify that independent engineers and operators can use them without undocumented knowledge.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L09: Seven complete reviewed frozen-candidate simulation records under the existing execution plan.

**Acceptance authority:** Documentation/release leads, independent engineers and operators. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Final production settings, commands, named roles, complete artifact set and independent specification/operator review remain open.

**Decision and record dependencies:** D07-Q01, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 681–711](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 757–801](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 1297–1347](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 2063–2113](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g33"></a>
## G33 — Developer onboarding

**Status: PARTIAL**

**Requirement:** Provide functional developer onboarding for the frozen network, SDK, CLI and documented interfaces. Verify clean setup, account/key handling, signing, submission, receipts, failures and network selection.

**Completed work**

- Portable SDK source archive contains 106 files and passed 171 runtime plus 21 documentation tests without a sibling node checkout.
- Actual-engine native/browser client compatibility is recorded.
- Maintained wallet regression suite and reproducible generated assets now have local build evidence.
- A separate local ordinary CLI profile reuses unchanged command code and preserves default CLI behavior and Cargo.lock. Selected/default tests, packaging checks and exact native artifact/source identities passed.
- Implemented the explicit strict-local-mldsa65 SDK profile and graph guard while retaining default compatibility. Produced and verified standalone source and Linux binary bundles. Used the extracted Linux CLI through actual installed services.
- Executed the exact clean-extracted strict Linux CLI natively on Ubuntu/glibc2.39. Help/version and24 outer checks passed under temporary nonroot isolated units, with binary/library hashes and cleanup checked. This proves bounded startup compatibility only.
- The retained Linux SDK client completed ordinary submission and receipt checks against the actual native engine RPC endpoint, including successful execution after emergency resume.
- Added and locally checked the process-interface route for emergency receipt queries. Exact digest parsing and consensus query context now have evidence.

**Evidence**

- [decision-register/engine-wallet-qualification/ACCEPTANCE.json](decision-register/engine-wallet-qualification/ACCEPTANCE.json)
- [decision-register/engine-wallet-qualification/APPROVAL.json](decision-register/engine-wallet-qualification/APPROVAL.json)
- [decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json](decision-register/engine-wallet-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/engine-wallet-qualification/REPORT.md](decision-register/engine-wallet-qualification/REPORT.md)
- [decision-register/engine-wallet-qualification/TEST_RESULTS.json](decision-register/engine-wallet-qualification/TEST_RESULTS.json)
- [decision-register/engine-wallet-qualification/VALIDATION.json](decision-register/engine-wallet-qualification/VALIDATION.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)
- [decision-register/ordinary-client-compatibility/ACCEPTANCE.json](decision-register/ordinary-client-compatibility/ACCEPTANCE.json)
- [decision-register/ordinary-client-compatibility/APPROVAL.json](decision-register/ordinary-client-compatibility/APPROVAL.json)
- [decision-register/ordinary-client-compatibility/REPORT.md](decision-register/ordinary-client-compatibility/REPORT.md)
- [decision-register/ordinary-client-compatibility/TEST_RESULTS.json](decision-register/ordinary-client-compatibility/TEST_RESULTS.json)
- [decision-register/ordinary-client-compatibility/VALIDATION.json](decision-register/ordinary-client-compatibility/VALIDATION.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/production-boundary-qualification/sdk/REPORT.md](decision-register/production-boundary-qualification/sdk/REPORT.md)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/sdk/REPORT.md](decision-register/linux-release-authority-qualification/sdk/REPORT.md)
- [decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md](decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_REPORT.md)
- [decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_USE_QUALIFICATION.json](decision-register/linux-release-authority-qualification/sdk/LINUX_BUNDLE_USE_QUALIFICATION.json)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/staging/RESULTS.json](decision-register/native-staging-production-closure/staging/RESULTS.json)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/integration/RESULTS.json](decision-register/emergency-upgrade-execution/integration/RESULTS.json)
- [decision-register/emergency-upgrade-execution/consensus/REPORT.md](decision-register/emergency-upgrade-execution/consensus/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Publish-ready artifacts must match the accepted source, dependencies and interface contract.
- Have an independent developer follow clean-environment instructions using the final network and clients.
- Qualify account onboarding, endpoint trust, receipt/state proofs and documented failure/recovery paths.

**Acceptance requirement:** Provide functional developer onboarding for the frozen network, SDK, CLI and documented interfaces. Verify clean setup, account/key handling, signing, submission, receipts, failures and network selection.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L07: Frozen hosted wallet and independent RPC evidence, receipt verification and complete supply reconciliation.

**Acceptance authority:** SDK lead and independent developer reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Changed Rust code has local executable evidence. Current Linux and actual-engine qualification must be repeated. Independent onboarding, accepted client distribution, endpoint trust, state/receipt proofs and hosted developer flows remain open.

**Decision and record dependencies:** D07-Q01, D10-Q01, D10-Q02, D10-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 273–341](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 803–857](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 2115–2155](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.

<a id="g34"></a>
## G34 — Validator onboarding

**Status: PARTIAL**

**Requirement:** Onboard verified validator operators with accepted roles, custody, funded stake, key proofs and infrastructure. Demonstrate clean deployment, admission, synchronization, participation, rotation, exit and recovery using approved procedures.

**Completed work**

- Permissioned registration, new-key proof, H+2 activation and rotation passed local lifecycle checks.
- Operator and infrastructure record packets identify missing production fields.
- Maintained service supervision and runbook sequence provide local preparation evidence.
- Recorded the supplied validator-operator nomination for Rick Glenn, CTO at Dytallix. Typed input checks now validate supplied public validator/peer mappings and funded delegations. Read-only discovery reconfirmed three Hetzner hosts and 21 foundation hashes; it found no accepted operator records.
- The existing role nominations now have explicit appointment/independence input fields in the seven-question packet. Linux tests demonstrate nonroot process operation, not accepted human operator appointments.
- Prepared concrete role, control-group, root custody and topology proposals under the existing question IDs. The review validator checks required public evidence and declared separation but cannot appoint operators or accept records.
- Prepared evidence-bound production input records and verified local installed service behavior. Preserved all missing production fields and human approvals. No operator appointment, custody acceptance or production onboarding was fabricated.
- Refreshed Hetzner asset evidence and production decision dependencies while preserving operator nominations and missing appointments. Shared asset availability does not establish approved validator placement or independent controllers.

**Evidence**

- [batch-8/implementation/APPROVAL.json](batch-8/implementation/APPROVAL.json)
- [batch-8/implementation/REPORT.md](batch-8/implementation/REPORT.md)
- [batch-8/implementation/TEST_RESULTS.json](batch-8/implementation/TEST_RESULTS.json)
- [batch-8/implementation/VALIDATION.json](batch-8/implementation/VALIDATION.json)
- [decision-register/gate-closure-implementation/services/REPORT.md](decision-register/gate-closure-implementation/services/REPORT.md)
- [decision-register/gate-closure-implementation/services/RESULTS.json](decision-register/gate-closure-implementation/services/RESULTS.json)
- [decision-register/gate-closure-implementation/services/VALIDATION.json](decision-register/gate-closure-implementation/services/VALIDATION.json)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)
- [decision-register/launch-preparation-workstreams/runbook/REPORT.md](decision-register/launch-preparation-workstreams/runbook/REPORT.md)
- [decision-register/launch-preparation-workstreams/runbook/RESULTS.json](decision-register/launch-preparation-workstreams/runbook/RESULTS.json)
- [decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json](decision-register/parallel-tracks-20260912/track-4/EVIDENCE_MANIFEST.json)
- [decision-register/parallel-tracks-20260912/track-4/REPORT.md](decision-register/parallel-tracks-20260912/track-4/REPORT.md)
- [decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json](decision-register/parallel-tracks-20260912/track-4/TEST_RESULTS.json)
- [decision-register/production-integration/records/ROLE_NOMINATIONS.json](decision-register/production-integration/records/ROLE_NOMINATIONS.json)
- [decision-register/production-integration/records/bindings/RESULTS.json](decision-register/production-integration/records/bindings/RESULTS.json)
- [decision-register/production-integration/records/discovery/REPORT.md](decision-register/production-integration/records/discovery/REPORT.md)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/records/RESULTS.json](decision-register/production-boundary-qualification/records/RESULTS.json)
- [decision-register/production-boundary-qualification/records/DECISION_QUEUE.md](decision-register/production-boundary-qualification/records/DECISION_QUEUE.md)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/records/DECISION_QUEUE.md](decision-register/production-profile-finalization/records/DECISION_QUEUE.md)
- [decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md](decision-register/production-profile-finalization/records/ROOT_AUTHORITY_CONTRACT.md)
- [decision-register/production-profile-finalization/records/RESULTS.json](decision-register/production-profile-finalization/records/RESULTS.json)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/records/REPORT.md](decision-register/linux-release-authority-qualification/records/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/decisions/REPORT.md](decision-register/native-staging-production-closure/decisions/REPORT.md)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Complete the initial operator set, verify control groups and keys, fund approved stake and obtain signed custody/service acceptance.
- Approve topology, admission thresholds and registry authority.
- Have named operators execute clean Linux onboarding and recovery against the frozen artifacts.

**Acceptance requirement:** Onboard verified validator operators with accepted roles, custody, funded stake, key proofs and infrastructure. Demonstrate clean deployment, admission, synchronization, participation, rotation, exit and recovery using approved procedures.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L01: Verified preparable fields; approved input policy and role assignments. Final candidate, production-key and genesis bindings remain pending until L11-L13.
- L05: Host identities, configuration digests, restricted services, backup/restore proof and alert delivery acknowledgement.
- L12: Authenticated approved distribution, signer/revocation checks and per-host exact artifact/config/genesis equality.

**Acceptance authority:** Validator/SRE leads, operators and independent onboarding reviewer. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Initial operator membership/control groups, accepted role/custody records, approved topology/admission rules and independent reviewer appointments remain incomplete.

**Decision and record dependencies:** D09-Q01, D09-Q02, D09-Q03, D09-Q04, D09-Q05, D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [USER_LAUNCH_REQUIREMENTS.txt lines 343–417](USER_LAUNCH_REQUIREMENTS.txt)
- [USER_LAUNCH_REQUIREMENTS.txt lines 757–801](USER_LAUNCH_REQUIREMENTS.txt)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Current source boundary:** Current source and saved evidence are checked in decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json. The root-helper change has native local tests; the prior Linux application predates it. Native Hetzner qualification in this queue covers strict CLI startup only. No full native validator staging or production acceptance is claimed.

<a id="g35"></a>
## G35 — No quantum-vulnerable asymmetric cryptography inside the production trust boundary

**Status: PARTIAL**

**Requirement:** No quantum-vulnerable asymmetric cryptography may establish or protect trust inside the production trust boundary.

**Completed work**

- Approved profile fixes operational ML-DSA-65, peer ML-KEM-768 and separate SLH-DSA root roles.
- Selected engine build excludes Comet classical key types, SecretConnection, libp2p/Noise and remote signer paths. Later cleanup removed inactive gRPC, SQL, profiling, Prometheus HTTP listener and explicit RPC TLS paths.
- Earlier HTTP/TLS engine inventories record 20 prohibited packages/1,977 symbol matches on macOS and 19 packages/1,919 matches on Linux. These are historical inventory matches, not unique algorithm counts or the new IPC artifact result.
- Actual-engine compatibility, maintained root validation and same-host engine/WASM reproduction provide bounded evidence.
- The explicit combined-tag Go IPC engine excludes net/http, crypto/tls, classical asymmetric packages, WebSocket/CORS, Viper/Afero and Prometheus from its selected graph. macOS and Linux ARM64 artifacts each have zero detected prohibited package/symbol matches and byte-identical same-host rebuilds.
- Internal review found only disabled stubs, ABI declarations and return-only markers in the two selected Go provider packages. Existing checker FAIL/NOT_GRANTED results are preserved pending acceptance.
- The new Hyper path passed 160 checks through four actual engines. The development root genesis path passed separate atomic-state and actual application-CLI checks. Neither result grants production acceptance.
- Inspection of the newly built Rust application still confirms three ring P-256 text symbols and a Security.framework link. This is a current compiled-artifact blocker, not only a source dependency concern.
- The exact Linux ARM64 application requires libssl.so.3 and libcrypto.so.3. The linked-provider record hashes both image-specific library sets and confirms exported RSA, ECDSA, ECDH and TLS implementations. Zero targeted matches in the application symbol table do not remove this provider blocker.
- The reduced pqc-fips204-only application profile fails to compile because RPC code imports a disabled oracle module. The explicit default feature set builds and passes local service checks; it is not an approved production exclusion profile.
- Selected application, bridge and ordinary CLI builds remove the identified classical dependency paths. Exact native screens and x86_64 app/adapter library closure inspection found no targeted classical matches or OpenSSL links. Root-enabled native and Linux integration passed with builtin supervisor hashes.
- Removed recorded legacy PQC and development-87 implementations from the selected native application. The exact executable has zero matches for the recorded classical, pre-standard PQC, unselected ML-DSA and prohibited-provider patterns. ML-DSA-65 and both storage codec markers are present. Selected dependency graphs contain 114 native/113 Linux packages and only ML-DSA-65 FIPS 204 features. Native combined 182 consensus/client plus 6 transport checks passed; no new Linux execution or production acceptance occurred.
- Inspected the new selected Linux application, adapter and strict SDK artifacts. Recorded zero matches under the named prohibited-implementation rules. Classified disabled Go BoringCrypto stubs, no-op markers and generated metadata separately. Exercised exact selected artifacts under local systemd. Full provider closure remains incomplete, including an unavailable emulator mapping; no G35 acceptance is claimed.
- Executed the prior strict CLI natively with no network or chain state and verified four expected host library hashes. Full loaded-provider maps were not captured. Added and locally tested explicit root-helper scratch selection; production policy remains unset and the Linux app is not rebuilt. Required missing interfaces and root/validator behavior remain explicit.
- Added SLH-DSA emergency authorization and fail-closed helper handling to current development sources. Prior sealed Linux artifacts predate these changes and require rebuild and complete boundary qualification. G35 remains a mandatory launch veto.
- Rebuilt current application/helper Linux artifacts and completed finite dependency/symbol checks. Extended strict service configuration for the emergency verifier. No complete operating-system, provider, public-interface or custody acceptance is inferred. The pinned stack completed the native lifecycle fixture. Observed process/provider mappings remain bounded evidence with explicit collection gaps.
- Extended SLH-DSA emergency and upgrade consumers with exact signed bindings, replay checks and atomic migration. Production guards remain enabled. Current source and local evidence are pinned; no final binary acceptance is claimed.

**Evidence**

- [decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json](decision-register/gate-closure-implementation/engine-runtime/EVIDENCE_DIGESTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/REPORT.md](decision-register/gate-closure-implementation/engine-runtime/REPORT.md)
- [decision-register/gate-closure-implementation/engine-runtime/RESULTS.json](decision-register/gate-closure-implementation/engine-runtime/RESULTS.json)
- [decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json](decision-register/gate-closure-implementation/engine-runtime/evidence/ENGINE_RUNTIME.json)
- [decision-register/gate-closure-implementation/pqc/CANDIDATE.json](decision-register/gate-closure-implementation/pqc/CANDIDATE.json)
- [decision-register/gate-closure-implementation/pqc/REPORT.md](decision-register/gate-closure-implementation/pqc/REPORT.md)
- [decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json](decision-register/gate-closure-implementation/pqc/TEST_RESULTS.json)
- [decision-register/gate-closure-implementation/root/REPORT.md](decision-register/gate-closure-implementation/root/REPORT.md)
- [decision-register/gate-closure-implementation/root/RESULTS.json](decision-register/gate-closure-implementation/root/RESULTS.json)
- [decision-register/gate-closure-implementation/wallet/REPORT.md](decision-register/gate-closure-implementation/wallet/REPORT.md)
- [decision-register/gate-closure-implementation/wallet/RESULTS.json](decision-register/gate-closure-implementation/wallet/RESULTS.json)
- [decision-register/launch-preparation-workstreams/security/REPORT.md](decision-register/launch-preparation-workstreams/security/REPORT.md)
- [decision-register/launch-preparation-workstreams/security/RESULTS.json](decision-register/launch-preparation-workstreams/security/RESULTS.json)
- [decision-register/pqc-build-boundary/ACCEPTANCE.json](decision-register/pqc-build-boundary/ACCEPTANCE.json)
- [decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json](decision-register/pqc-build-boundary/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-build-boundary/REPORT.md](decision-register/pqc-build-boundary/REPORT.md)
- [decision-register/pqc-build-boundary/TEST_RESULTS.json](decision-register/pqc-build-boundary/TEST_RESULTS.json)
- [decision-register/pqc-build-boundary/VALIDATION.json](decision-register/pqc-build-boundary/VALIDATION.json)
- [decision-register/pqc-only-launch-gate/APPROVAL.json](decision-register/pqc-only-launch-gate/APPROVAL.json)
- [decision-register/pqc-only-launch-gate/REPORT.md](decision-register/pqc-only-launch-gate/REPORT.md)
- [decision-register/pqc-only-launch-gate/VALIDATION.json](decision-register/pqc-only-launch-gate/VALIDATION.json)
- [decision-register/pqc-profile/ACCEPTANCE.json](decision-register/pqc-profile/ACCEPTANCE.json)
- [decision-register/pqc-profile/APPROVAL.json](decision-register/pqc-profile/APPROVAL.json)
- [decision-register/pqc-profile/REPORT.md](decision-register/pqc-profile/REPORT.md)
- [decision-register/pqc-profile/TEST_RESULTS.json](decision-register/pqc-profile/TEST_RESULTS.json)
- [decision-register/pqc-profile/VALIDATION.json](decision-register/pqc-profile/VALIDATION.json)
- [decision-register/pqc-transport-integration/ACCEPTANCE.json](decision-register/pqc-transport-integration/ACCEPTANCE.json)
- [decision-register/pqc-transport-integration/EVIDENCE_DIGESTS.json](decision-register/pqc-transport-integration/EVIDENCE_DIGESTS.json)
- [decision-register/pqc-transport-integration/REPORT.md](decision-register/pqc-transport-integration/REPORT.md)
- [decision-register/pqc-transport-integration/TEST_RESULTS.json](decision-register/pqc-transport-integration/TEST_RESULTS.json)
- [decision-register/pqc-transport-integration/VALIDATION.json](decision-register/pqc-transport-integration/VALIDATION.json)
- [decision-register/production-integration/pqc/QUALIFICATION.json](decision-register/production-integration/pqc/QUALIFICATION.json)
- [decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json](decision-register/production-integration/pqc/PROVIDER_SOURCE_REVIEW.json)
- [decision-register/production-integration/pqc/http-adapter/RESULTS.json](decision-register/production-integration/pqc/http-adapter/RESULTS.json)
- [decision-register/production-integration/pqc/ipc-runtime/RESULTS.json](decision-register/production-integration/pqc/ipc-runtime/RESULTS.json)
- [decision-register/production-integration/root/RESULTS.json](decision-register/production-integration/root/RESULTS.json)
- [decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json](decision-register/production-integration/evidence/CURRENT_RUST_ARTIFACT_SCREEN.json)
- [decision-register/production-integration/evidence/FINAL_SOURCE_CHECK.json](decision-register/production-integration/evidence/FINAL_SOURCE_CHECK.json)
- [decision-register/production-integration/linux/retry/RESULTS.json](decision-register/production-integration/linux/retry/RESULTS.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json](decision-register/production-integration/linux/retry/evidence/LINUX_APPLICATION_INVENTORY.json)
- [decision-register/production-integration/linux/retry/evidence/LINUX_LINKED_PROVIDERS.json](decision-register/production-integration/linux/retry/evidence/LINUX_LINKED_PROVIDERS.json)
- [decision-register/production-integration/linux/retry/attempt-minimal-feature/RUST_BUILD.json](decision-register/production-integration/linux/retry/attempt-minimal-feature/RUST_BUILD.json)
- [decision-register/production-boundary-qualification/REPORT.md](decision-register/production-boundary-qualification/REPORT.md)
- [decision-register/production-boundary-qualification/RESULTS.json](decision-register/production-boundary-qualification/RESULTS.json)
- [decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json](decision-register/production-boundary-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json](decision-register/production-boundary-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/production-boundary-qualification/rust/RESULTS.json](decision-register/production-boundary-qualification/rust/RESULTS.json)
- [decision-register/production-boundary-qualification/sdk/RESULTS.json](decision-register/production-boundary-qualification/sdk/RESULTS.json)
- [decision-register/production-boundary-qualification/linux-x86/REPORT.md](decision-register/production-boundary-qualification/linux-x86/REPORT.md)
- [decision-register/production-boundary-qualification/integration/RESULTS.json](decision-register/production-boundary-qualification/integration/RESULTS.json)
- [decision-register/production-boundary-qualification/review/RUST_PROFILE_REVIEW.md](decision-register/production-boundary-qualification/review/RUST_PROFILE_REVIEW.md)
- [decision-register/production-profile-finalization/REPORT.md](decision-register/production-profile-finalization/REPORT.md)
- [decision-register/production-profile-finalization/RESULTS.json](decision-register/production-profile-finalization/RESULTS.json)
- [decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json](decision-register/production-profile-finalization/EVIDENCE_DIGESTS.json)
- [decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json](decision-register/production-profile-finalization/evidence/SOURCE_CHECK.json)
- [decision-register/production-profile-finalization/crypto/RESULTS.json](decision-register/production-profile-finalization/crypto/RESULTS.json)
- [decision-register/production-profile-finalization/crypto/selected-graph.json](decision-register/production-profile-finalization/crypto/selected-graph.json)
- [decision-register/production-profile-finalization/crypto/selected-linux-graph.json](decision-register/production-profile-finalization/crypto/selected-linux-graph.json)
- [decision-register/production-profile-finalization/evidence/ARTIFACT_INSPECTION.json](decision-register/production-profile-finalization/evidence/ARTIFACT_INSPECTION.json)
- [decision-register/production-profile-finalization/evidence/PINNED_NATIVE_ARCHIVES.json](decision-register/production-profile-finalization/evidence/PINNED_NATIVE_ARCHIVES.json)
- [decision-register/production-profile-finalization/integration/native01/RUNTIME.json](decision-register/production-profile-finalization/integration/native01/RUNTIME.json)
- [decision-register/production-profile-finalization/records/DECISION_QUEUE.md](decision-register/production-profile-finalization/records/DECISION_QUEUE.md)
- [decision-register/linux-release-authority-qualification/REPORT.md](decision-register/linux-release-authority-qualification/REPORT.md)
- [decision-register/linux-release-authority-qualification/RESULTS.json](decision-register/linux-release-authority-qualification/RESULTS.json)
- [decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json](decision-register/linux-release-authority-qualification/EVIDENCE_DIGESTS.json)
- [decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json](decision-register/linux-release-authority-qualification/evidence/SOURCE_CHECK.json)
- [decision-register/linux-release-authority-qualification/linux/REPORT.md](decision-register/linux-release-authority-qualification/linux/REPORT.md)
- [decision-register/linux-release-authority-qualification/sdk/REPORT.md](decision-register/linux-release-authority-qualification/sdk/REPORT.md)
- [decision-register/linux-release-authority-qualification/providers/REPORT.md](decision-register/linux-release-authority-qualification/providers/REPORT.md)
- [decision-register/linux-release-authority-qualification/services/REPORT.md](decision-register/linux-release-authority-qualification/services/REPORT.md)
- [decision-register/linux-release-authority-qualification/root/REPORT.md](decision-register/linux-release-authority-qualification/root/REPORT.md)
- [decision-register/native-staging-production-closure/REPORT.md](decision-register/native-staging-production-closure/REPORT.md)
- [decision-register/native-staging-production-closure/RESULTS.json](decision-register/native-staging-production-closure/RESULTS.json)
- [decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json](decision-register/native-staging-production-closure/EVIDENCE_DIGESTS.json)
- [decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json](decision-register/native-staging-production-closure/evidence/SOURCE_CHECK.json)
- [decision-register/native-staging-production-closure/staging/REPORT.md](decision-register/native-staging-production-closure/staging/REPORT.md)
- [decision-register/native-staging-production-closure/root-helper/REPORT.md](decision-register/native-staging-production-closure/root-helper/REPORT.md)
- [decision-register/native-staging-production-closure/behavior/REPORT.md](decision-register/native-staging-production-closure/behavior/REPORT.md)
- [decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md](decision-register/native-staging-production-closure/root/INTEGRATION_CONTRACT.md)
- [decision-register/native-staging-production-closure/decisions/REPORT.md](decision-register/native-staging-production-closure/decisions/REPORT.md)
- [decision-register/emergency-transaction-freeze/REPORT.md](decision-register/emergency-transaction-freeze/REPORT.md)
- [decision-register/emergency-transaction-freeze/RESULTS.json](decision-register/emergency-transaction-freeze/RESULTS.json)
- [decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json](decision-register/emergency-transaction-freeze/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json](decision-register/emergency-transaction-freeze/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-transaction-freeze/verifier/REPORT.md](decision-register/emergency-transaction-freeze/verifier/REPORT.md)
- [decision-register/emergency-transaction-freeze/integration/REPORT.md](decision-register/emergency-transaction-freeze/integration/REPORT.md)
- [decision-register/emergency-transaction-freeze/review/REVIEW.md](decision-register/emergency-transaction-freeze/review/REVIEW.md)
- [decision-register/emergency-release-staging/REPORT.md](decision-register/emergency-release-staging/REPORT.md)
- [decision-register/emergency-release-staging/RESULTS.json](decision-register/emergency-release-staging/RESULTS.json)
- [decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json](decision-register/emergency-release-staging/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json](decision-register/emergency-release-staging/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-release-staging/linux/REPORT.md](decision-register/emergency-release-staging/linux/REPORT.md)
- [decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json](decision-register/emergency-release-staging/linux/evidence/ARTIFACT_HANDOFF.json)
- [decision-register/emergency-release-staging/service/REPORT.md](decision-register/emergency-release-staging/service/REPORT.md)
- [decision-register/emergency-release-staging/native/REPORT.md](decision-register/emergency-release-staging/native/REPORT.md)
- [decision-register/emergency-release-staging/native/RESULTS.json](decision-register/emergency-release-staging/native/RESULTS.json)
- [decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md](decision-register/emergency-release-staging/review/NATIVE_TOOL_REVIEW.md)
- [decision-register/emergency-release-staging/review/VALIDATION.json](decision-register/emergency-release-staging/review/VALIDATION.json)
- [decision-register/emergency-upgrade-execution/REPORT.md](decision-register/emergency-upgrade-execution/REPORT.md)
- [decision-register/emergency-upgrade-execution/RESULTS.json](decision-register/emergency-upgrade-execution/RESULTS.json)
- [decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json](decision-register/emergency-upgrade-execution/EVIDENCE_DIGESTS.json)
- [decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json](decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json)
- [decision-register/emergency-upgrade-execution/emergency/REPORT.md](decision-register/emergency-upgrade-execution/emergency/REPORT.md)
- [decision-register/emergency-upgrade-execution/verifier/REPORT.md](decision-register/emergency-upgrade-execution/verifier/REPORT.md)
- [decision-register/emergency-upgrade-execution/upgrade/REPORT.md](decision-register/emergency-upgrade-execution/upgrade/REPORT.md)
- [decision-register/emergency-upgrade-execution/review/VALIDATION.json](decision-register/emergency-upgrade-execution/review/VALIDATION.json)

Exact SHA-256 values are in this gate's JSON evidence entries. Package result records identify commands, source/artifact hashes, logs and execution limits.

**Remaining work**

- Approve the full production algorithm/interface/storage profile and qualify the deployed distribution, OS, interpreter and provider boundary on approved hosts. Resolve the executable verified-helper scratch policy and complete provider evidence outside the emulator.
- Complete exact role/parameter/standards profiles, source/config/dependency inventories, loaded-library inspection and independently reproducible release hashes. Default compatibility source/builds remain outside the selected strict executable scope.
- Approve production root-required genesis schemas and manifest semantics. Integrate durable production upgrade/emergency consumers, action priority/cancellation and accepted custody, rotation and recovery.
- Qualify operational, account, peer, client, receipt-proof, restart and resource requirements against the exact frozen candidate, including approved x86_64 hosts and the hosted wallet.
- Obtain independent protocol, cryptography, release and operator acceptance for every required G35 evidence item.

**Acceptance requirement:** No quantum-vulnerable asymmetric cryptography may establish or protect trust inside the production trust boundary.

- Use the exact frozen source, binary/image, genesis and configuration hashes. Record the environment, operators, start/end times, procedure, raw evidence digest, reviewer and unresolved defects.
- Satisfy all gate-specific requirements and approved thresholds. Resolve affected P0 defects and absolute launch blockers. Requalify affected evidence after candidate changes.
- Obtain acceptance from named authorized owners and independent reviewers. Agent assessments and local test passes do not supply those signatures.
- L02: Frozen source, locks, build environment, executables, configuration templates and provider inventories; independent G35 acceptance. Final production genesis bytes are bound at L11.
- L03: Review scope, frozen candidate hashes, findings, remediation checks and signed acceptance.
- L10: Explicit ceremony authorization, public key validation, custody roles and recovery procedures. Never export private material.
- L12: Authenticated approved distribution, signer/revocation checks and per-host exact artifact/config/genesis equality.

**Acceptance authority:** Independent protocol, cryptography, release and operator reviewers. Named assignee, independent reviewer and acceptance record remain unset.

**Current blocker:** Mandatory veto remains active. Changed Rust paths need Linux rebuild and compiled-artifact inspection. Full OS/provider/distribution, interface/custody, production wire acceptance, independent reproduction and review remain incomplete.

**Decision and record dependencies:** D06-Q01, D06-Q02, D10-Q01, D10-Q02, D10-Q03, D12-Q01, D12-Q02, D12-Q03, D14-Q01, D14-Q02, D14-Q03. See exact questions and source status in this gate's JSON record.

**Requirement sources**

- [decision-register/pqc-profile/GATE_SPEC.json /requirement](decision-register/pqc-profile/GATE_SPEC.json)
- [MAINNET_EXECUTION_PLAN.md: Evidence and status rules; Week gates; Seven simulation runs](MAINNET_EXECUTION_PLAN.md)
- [decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json /steps](decision-register/launch-preparation-workstreams/runbook/CHECKLIST.json)

**Exact G35 required evidence**

- Versioned algorithm/parameter profile by role and exact standards revisions including applicable errata
- Source/configuration/dependency crypto bill of materials plus reproducible release artifact hashes and loaded-library inspection
- Complete PQC peer identity, authenticated key establishment and record-layer integration, with no prohibited fallback
- Operational signing, account/key migration, consensus, SDK, wallet, receipts, restart, RPC and light-client tests against the exact candidate
- Separate root authorization with trusted keys, policy, replay persistence, custody, rotation and actual genesis/upgrade/emergency execution checks
- Interoperability, known-answer, malformed-input, downgrade-rejection, resource-limit and side-channel review evidence
- Independent protocol, cryptography, release and operator review tied to the exact source and artifact hashes

**Original formal pass rule:** All required evidence must be complete, independently accepted and current for the production candidate. Any unknown, failed check or missing integration keeps G35 FAIL and mainnet NO_GO.
The archived FAIL term denotes failed formal qualification. The current progress term is PARTIAL. The launch veto remains active.

**Current source boundary:** Current source is pinned in decision-register/emergency-upgrade-execution/evidence/SOURCE_CHECK.json. New Rust implementation has local executable evidence. Prior Linux/native artifacts precede these changes. Production and G35 acceptance remain open.
