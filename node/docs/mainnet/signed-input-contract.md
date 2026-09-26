# Signed input conversion contract

Base revision: ddfe1b2db04320790e061477c698a1f881f87f6f.
Scope: public signed submission and selected development block settlement and recovery.

The stored original signature must verify. The original transaction must match the stored
chain ID. One conversion function must reproduce every stored execution field and ordered
message. Selected settlement must check this before execution or a rejected receipt write.
Recovery must check every committed signed record against its original input and block body.

The node owns cryptographic verification. Storage owns its versioned record format and
structural links. The shared node conversion preserves case-insensitive token aliases,
checked 10^6 conversion, checked display totals, all five message types, and the existing
self-send recipient selection rule. Signature, public key, chain ID, memo, nonce, and total
fee retain their current values. Public error status and fee log fields remain unchanged.

Gas conversion uses the stored positive gas price and exact division of the signed total
fee. This is the existing development conversion rule. The original signature does not
choose a gas price. A frozen mainnet policy and authenticated gas limits remain unresolved.
Unsigned internal development records retain their current execution rules. Public signed
submission still requires a valid original signature. Legacy records require migration.

This change does not establish signer ownership, complete signed envelope commitments,
mainnet address policy, distributed replay, cryptographic interoperability, or mainnet
readiness. Reverification adds cryptographic work to the existing full-history recovery
pass, which also runs before block planning. Production throughput remains unqualified.
No exploit reproduction or live network test is part of this batch.

Read-only source investigation confirmed the conversion and compatibility boundary before
implementation. Tests must cover original signature verification, complete converted record
comparison, aliases, message order, exact arithmetic, stored gas price, and ordinary signed
submission through settlement and restart. The full workspace test command is a release
check; existing failures do not count as a pass.
