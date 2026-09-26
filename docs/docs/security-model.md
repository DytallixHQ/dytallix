# Security Model

This page summarizes the Dytallix security posture from the public SDK, node
snapshot, and appendix materials.

## Cryptographic Model

Public Dytallix accounts use ML-DSA-65.

Key points:

- addresses are derived from ML-DSA-65 public keys
- addresses are encoded as Bech32m D-Addrs
- the D-Addr payload is a 32-byte hash of the public key
- normal public-account workflows do not use legacy or hybrid account modes

The CLI also exposes `SLH-DSA` generation as an explicit advanced option, but
the canonical public account and address model remains ML-DSA-65.

## Security Assumptions

The companion appendix material makes the protocol assumptions explicit. In
plain terms, Dytallix security depends on:

- post-quantum primitives remaining secure at the chosen parameter sets
- fewer than one-third of staked governance weight being Byzantine in the BFT
  safety model
- bounded network delay assumptions for finality and coordination
- validators behaving rationally often enough for the economic mechanisms to
  work as intended

## Operational Security

The published node snapshot emphasizes key-handling hygiene:

- no plaintext validator private keys should be persisted by the node
- validator material may be loaded from Vault KV v2
- sealed local keystores are the fallback path

This is relevant both for operators and for audit-readiness documentation.

## Threat Categories Called Out In The Appendix

The appendix material highlights several major threat classes:

- external passive observation
- external active spam and DoS
- minority Byzantine validators
- economic cartel behavior
- gateway censorship or selective inclusion
- MPC custody or bridge-related key compromise

The mitigation language across those materials includes:

- dimensional gas and fee pressure
- slashing and quorum rules
- outlier handling for oracle-style inputs
- bonding and auditability for gateway-like components

## Current Public-Facing Reality

For developers integrating today, the most relevant practical security facts are:

- accounts are PQC-native
- signatures are ML-DSA-65 in the public SDK flow
- balances, nonces, and receipts are available through public read endpoints
- transaction submission expects a signed envelope rather than raw legacy fields

## Research And Experimental Modules

The local node snapshot also contains additional research or draft modules, such
as:

- PulseGuard risk-scoring components
- bridge quorum handling
- oracle-related extensions
- some multi-algorithm PQC notes

These are useful for understanding the broader protocol direction, but they are
not all equally mature as public developer interfaces.

## Security Guidance For Builders

- validate D-Addrs locally before submitting anything
- read `nonce` from the network before signing
- read `/status` before deriving fee assumptions
- do not infer public endpoint availability from old `/v1/*` docs alone
- prefer the live gateway and published SDK behavior over placeholder comments
