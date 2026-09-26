# Address format proposal

Status: DRAFT NOT FROZEN. The format is a recommendation. No mainnet identity policy is approved by this file.

The current working tree contains a versioned node address codec. It encodes a
34-byte payload: version 1, account kind 1, and a 32-byte account identifier.
The codec uses Bech32m and lowercase network prefixes: `dytallix` for mainnet,
`tdytallix` for testnet, and `ddytallix` for development. These identifiers remain
proposed until the protocol decision is approved.

At account creation, `AccountAddress::from_origin_key` derives the identifier with
SHA3-256. Its input binds the origin domain, network code, chain ID, algorithm
code, and public key. Derive this identifier once. Keep it unchanged when an
authorized key changes. Store the current key in persistent authorization state.
The codec does not implement that state or prove key ownership.

The SDK still uses an unversioned 32-byte BLAKE3 public-key hash with the
`dytallix` prefix. The SDK and patched node formats are incompatible. The old
node hexadecimal helpers remain historical compatibility code. Do not convert
an address by changing its prefix, adding version bytes, or copying balances.

`crates/protocol-types/src/address.rs` defines the current candidate codec.
`crates/protocol-types/tests/fixtures/address-v1.json` contains 12 encoding,
9 origin, and 10 invalid-input vectors. Origin vectors use synthetic byte
patterns. They are not signature known-answer tests or valid-key evidence.
`scripts/address_reference.py` checks the fixture without changing it.

Before mainnet, approve one identity and algorithm contract. Implement persistent
account authorization, authorized rotation, preconfigured recovery, and atomic
state updates. Reconcile node, SDK, wallet, genesis, and validator startup.
Reject unsupported algorithms, wrong networks, obsolete keys, and replayed
updates. Require one signed migration record for any approved legacy allocation.

The complete Batch 2 recommendation and closure tests are recorded in
`/Users/rickglenn/Developer/Dytallix-mainnet-launch/batch-2/IDENTITY_DECISION.md`
and `/Users/rickglenn/Developer/Dytallix-mainnet-launch/PQC_ARCHITECTURE.md`.
These local planning references are not portable release references.

A checksum detects input errors. It does not authenticate a sender. An address
codec does not establish post-quantum security or a validated cryptographic module.
