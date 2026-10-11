# Dytallix mainnet launch tracking

Launch: **NO GO**. The [G01–G35 gate list](MAINNET_GATE_MASTER.md) is the readiness record. It is generated from [LAUNCH_GATES.json](LAUNCH_GATES.json) by `node/tools/mainnet-preparation/gate_master.py`, and CI checks that the two match. All 35 gates are PARTIAL; none is ready for acceptance.

## Start here

- [Genesis framework](GENESIS_FRAMEWORK.md): what genesis contains, who controls it, and the order of work to launch
- [Trust model](TRUST_MODEL.md): what anyone using the chain trusts at launch
- [Decision register](DECISIONS_REQUIRED.md), with its data in [MAINNET_DECISION_REGISTER.json](MAINNET_DECISION_REGISTER.json) and the dated P01 approvals in [approvals/](approvals/)
- [E05 intake](E05_INTAKE.md) and [E05 values](E05_VALUES.json): production values and records
- [Execution plan](MAINNET_EXECUTION_PLAN.md): scope, the stages a candidate passes, the seven simulation runs and the evidence rules

## Specifications and models

These drafts are being brought up to date; where they differ from the documents above, those are right.

- [Mainnet v1 specification](MAINNET_V1_SPEC.md) and [PQC architecture](PQC_ARCHITECTURE.md)
- [DGT](DGT_TOKENOMICS.md), [DRT](DRT_TOKENOMICS.md) and the [token supply model](TOKEN_SUPPLY_MODEL.md)
- [Launch requirements](USER_LAUNCH_REQUIREMENTS.txt): the original requirements the gates cite

## Records

- **Custody:** the [key ceremony](custody/KEY_CEREMONY.md) and the [emergency](custody/emergency/INTAKE.md), [upgrade](custody/upgrade/INTAKE.md) and [genesis](custody/genesis/INTAKE.md) intakes
- **Genesis:** the [network identity](genesis/IDENTITY.json), the [proposals](genesis/PROPOSALS.json) and the [genesis builder](../node/docs/mainnet/e05-genesis-builder.md)
- **Hosts:** the [setup values](hosts/SETUP_VALUES.json), the [pin plan template](hosts/PIN_PLAN.template.json), the [proposals](hosts/PROPOSALS.json) and [host setup v1](../node/docs/architecture/host-setup-v1.md)
- **Operations:** the [objectives](operations/OBJECTIVES.md) and the [runbooks](../node/docs/operations/README.md)
- **Release:** the [reproducible release](../release/README.md)

## Historical records

These keep their September 2026 content, each with a banner naming the documents that now serve its role: the [Day 0 status](DAY_0_STATUS.md), the [readiness report](MAINNET_READINESS_REPORT.md), the [artifact gap report](MAINNET_ARTIFACT_GAP_REPORT.md) and [artifact register](MAINNET_ARTIFACT_REGISTER.json), the [genesis specification draft](GENESIS_SPEC.md), the [genesis manifest](GENESIS_MANIFEST.json), the [genesis allocation draft](GENESIS_ALLOCATION_DRAFT.md) ([JSON](GENESIS_ALLOCATION_DRAFT.json)), the [validator architecture](VALIDATOR_ARCHITECTURE.md) and [validator operator register](VALIDATOR_OPERATOR_REGISTER.json), and the [production infrastructure draft](PRODUCTION_INFRASTRUCTURE_DRAFT.md) ([JSON](PRODUCTION_INFRASTRUCTURE_DRAFT.json)).

They and the gate data name evidence records under `decision-register/`, `batch-*/`, `evidence/`, `snapshots/` and `assessment/`, which are held outside this repository and appear as text, not links ([launch records](approvals/P01_E05_LAUNCH_RECORDS_2026-10-10.json)).

Artifact delivery, local test passes and policy approval are not gate acceptance. Nothing here grants launch authority or performs a deployment, key operation, genesis ceremony or release.
