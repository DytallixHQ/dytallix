# Changelog

## Unreleased

- removed the CosmWasm bridge (`cosmos_bridge`, `cosmos_bridge_optimized`) and `storage_optimizer`: `cosmwasm-std` pulled in classical signature code (k256, ecdsa, ed25519-zebra), and Dytallix has no classical public-key cryptography (E04 gap 19)
- the crate is now a workspace of its own
- imported and cleaned the published Dytallix smart-contract crate into the dedicated repository
- added reference `staking`, `governance`, and `algorithm_registry` modules
- fixed inherited library test failures in gas-analysis and storage-optimization logic
- rewrote the repository README and added repo-local docs
- added standalone `counter`, `reward_splitter`, and `algorithm_guard` example contracts
- added contributor docs, security policy, CI workflow, and development Make targets

