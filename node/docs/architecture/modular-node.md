# Modular node architecture

Status: approved direction; implementation in progress. This document does not approve mainnet.

## Source authority

The dytallix-node workspace is the authoritative source for the mainnet executable and its protocol-critical components. The embedded pqc-crypto and smart-contracts paths identify workspace component sources. The release graph must establish which paths the selected binary actually links. Standalone repositories remain compatibility distributions until their differences receive explicit disposition. Do not copy a standalone implementation into the node without review.

New reusable components live under crates/. Their versions begin at 0.1.0 and publish=false until release qualification. Existing node module paths remain compatibility exports. Use one executable to compose the modules. Compile-time features select supported implementations; they do not authorize different validation rules among validators.

## Module contracts

| Module | Inputs | Outputs and owned behavior | Dependency rule |
|---|---|---|---|
| protocol-types | Existing transaction fields | Msg and Tx values, canonical JSON, transaction hashes | No runtime, storage, network, signing provider, or node dependency |
| runtime-crypto | Explicit key, message, signature, algorithm | Existing signature and verification results | May use protocol-types and selected cryptographic providers |
| storage | Existing blocks, transactions, receipts, configured database path | Existing database records and retrieval API | May use component value types; no node or RPC dependency |
| gas | Explicit schedule, limit, operation cost | Meter state, gas totals, existing errors | No production storage, network, cryptography, or node dependency |
| consensus (target) | Authenticated proposals, votes, validator set, protocol time | Ordered finalized transitions and consensus state | No RPC, external oracle requests, or application database escape path |
| execution (target) | Prior state, ordered transaction, approved protocol version | Atomic state changes, receipt, events | Call restricted state and accounting interfaces |
| economics (target) | Approved parameters, prior controller/accounting state, validated observations | Deterministic mint, burn, fee, reward updates | No local time, floating-point consensus values, direct network, or uncontrolled randomness |
| node services | Operator configuration, peer and client traffic | Composition, RPC, admission, monitoring | May compose modules; lower modules must not depend on node services |

The extracted storage crate initially preserves existing public database access and optional bridge/oracle helpers. It is a compatibility boundary, not completed capability isolation. Its current write sequence is not claimed to be atomic. A database reopen test does not prove crash recovery or finalized-state continuity.

The gas crate preserves the existing schedule. It does not implement or approve the paper's economic model. Protocol-types preserves current debug output and encoding. The current JSON codec is the chain's existing codec; this extraction does not claim an external canonical-JSON standard.

## Versions and upgrades

Track four independent versions: public API, wire format, persisted state, and consensus rules. Rust package versions do not substitute for consensus versions. This extraction changes package organization only. Transaction version 1, receipt version 1, gas table version 1, algorithms, and existing data formats remain unchanged.

For a future consensus change, approve the rule version, activation height, source and artifact digests, old/new compatibility interval, and state migration. Validators must use the same active rules. Record unknown-version behavior. Test the migration against a retained state snapshot before scheduling activation. Never infer mainnet activation from a local feature flag.

Cryptographic transitions must identify algorithm and parameter set, key and signature encodings, allowed uses, activation and retirement points, and historical verification policy. Do not silently substitute an unsupported algorithm. The current dilithium5 compatibility label remains unchanged in this extraction; its precise backend mapping remains documented separately.

## State access and accounting target

Introduce narrow interfaces from actual callers after extracting the current implementations. A read view must not write. A transition writer must collect all related changes for one atomic commit. Route asset supply and balance changes through one accounting owner. Define mint, burn, transfer, reward accrual, claim, rounding, overflow, and failure behavior in the approved specification before changing those rules.

The economic controller interface will accept explicit prior state, validated observations, protocol time, and approved parameters. It will return new controller state and proposed issuance. A separate accounting transition will apply approved issuance. Parameter bounds and output clipping do not establish stability. D01 through D07 in docs/mainnet/specification-decisions.md remain unresolved.

## Component acceptance

Each component needs a named owner, public interface, dependency allowlist, compatibility vectors, error behavior, resource limits, and release evidence. Each stateful component also needs migration and recovery tests. Keep the old node import path until supported callers migrate. Do not maintain a second hand-edited implementation behind that path.

Required checks include the workspace suite, old import-path compatibility tests, fixed transaction bytes and hashes, supported backend builds, key lifecycle checks, and state reopen checks. Run the independent patch review for cryptographic corrections. Dependency policy checks enforce declared crate edges; they do not prove semantic determinism or the absence of every I/O operation.

## Remaining sequence

1. Complete the initial extraction and correct the four recorded embedded PQC failures.
2. Reconcile standalone component differences, preserve supported APIs, and produce generated distributions from the authoritative source.
3. Approve D01 through D07, numerical parameters, proof obligations, and launch features.
4. Replace direct state access with atomic execution and accounting interfaces; retain state compatibility tests.
5. Implement approved distributed consensus and economics. Qualify cryptographic transitions and state migrations.
6. Run independent security review, release-candidate testnet, operator recovery drills, and genesis approval.

## Initial verification exclusions

The full workspace command also builds blockchain-core research tests. Its ai_request_payload_test currently uses constructors and fields that do not match the current AIRequestMetadata API. This is separate from the extracted crates. Do not suppress this test or infer distributed consensus readiness from the selected-node build.

The complete node test run also exposes governance, metrics, and mempool expectations that require disposition. Some gas fixtures assumed fees were charged in udgt, while the existing runtime charges udrt. This extraction updates those fixtures and retains explicit fee and transfer assertions. It does not change economic runtime behavior. Resolve remaining protocol assertions against the approved specification.
