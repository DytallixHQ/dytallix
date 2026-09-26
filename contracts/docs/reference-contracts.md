# Reference Contracts

## Tokenomics

- [`src/tokenomics/dgt_token.rs`](../src/tokenomics/dgt_token.rs) defines the fixed-supply governance token.
- [`src/tokenomics/drt_token.rs`](../src/tokenomics/drt_token.rs) defines the reward token with burn support and emission-controller hooks.
- [`src/tokenomics/emission_controller.rs`](../src/tokenomics/emission_controller.rs) manages emission parameters, adaptive rate calculation, and pool accounting.

## Staking

[`src/staking.rs`](../src/staking.rs) provides:

- validator registration with self-bond requirements
- delegation and undelegation flows
- reward-index accounting for O(1) reward settlement
- slashing and validator status updates
- direct voting-power queries per validator

## Governance

[`src/governance.rs`](../src/governance.rs) provides:

- deposit-gated proposal creation
- DGT-weighted vote accounting
- quorum, veto, and pass-threshold checks
- timelock before execution
- execution routing for tokenomics, parameter, treasury, and custom actions

## Algorithm Registry

[`src/algorithm_registry.rs`](../src/algorithm_registry.rs) provides:

- algorithm registration by owner or guardian
- lifecycle transitions: active, deprecated, revoked, suspended
- capability-aware allow/deny checks
- emergency circuit breaker support

## Runtime And Utilities

- [`src/runtime.rs`](../src/runtime.rs) for WASM deployment and execution
- [`src/security/gas_attack_analyzer.rs`](../src/security/gas_attack_analyzer.rs) for gas-abuse detection
- [`src/storage_optimizer.rs`](../src/storage_optimizer.rs) for caching, compression, and storage recommendations
- [`src/gas_optimizer.rs`](../src/gas_optimizer.rs) for gas-cost estimation support

