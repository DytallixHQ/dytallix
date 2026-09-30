# E05-a: genesis capacity and configuration checks

Engineering task E05 (genesis from approved inputs), step a. Before the
production values are chosen, the node must accept a genesis of launch size
and refuse values that are individually valid but make the chain unusable.
This step selects no production value. Paths are relative to
`dytallix-fast-launch/node/src/`.

The production-value inventory behind this step lists 201 configuration and
genesis values with their paths, code bounds and fixture values. It is
review input for the E05 intake packet, not a record.

## Problems

| ID | Problem | Effect |
| --- | --- | --- |
| G1 | The consensus configuration was capped at 65,536 bytes, and it carries every genesis account's recovery record, about 15 KB each (each ML-DSA-65 key appears twice as a JSON number array). Measured: 6,412 bytes with no accounts, 53,436 with three. | Genesis could hold at most three accounts; launch needs at least the five DGT buckets and four validator operators. |
| G2 | Governance thresholds could be 0. The pass rule requires `no_with_veto` below the veto weight, so a zero veto threshold fails every proposal; zero quorum or approval pass any vote. | Governance unusable or unguarded. |
| G3 | The fee cap was not checked against the largest fee a signer needs (`max_transaction_gas × gas_price`), in the ordinary and recovery profiles. Governance can raise the ordinary gas price, but not the cap. | Large transactions unsignable, more so after a governed price rise. |
| G4 | Genesis fee values, `min_self_bond` and `max_active` were not checked against their governance bounds, and the `max_active` bound not against the reward state's validator capacity. | Genesis outside its own bounds; a passed `max_active` proposal failing at execution. |
| G5 | The ordinary transport bound was not checked against the base64 size of a full envelope (`limits.max_wire_bytes`). | Full-size transactions unsendable. |
| G6 | Root control bounds (emergency, upgrade, handover) were not checked against their threshold's signatures, 59,584 hex characters each. | A threshold control that cannot be submitted. |
| G7 | Lifecycle evidence seconds had no bound below the engine's nanosecond duration range. | Refused only at chain start. |

## Rules

1. **Capacity.** The consensus configuration bound is 8 MiB
   (`consensus_settlement::MAX_CONFIG_BYTES`), the application genesis pipe
   bound, in every consumer: configuration validation, genesis
   initialization, the release preflight, `consensus_stdio`,
   `dytallix-state-check`, the supervisor's pinned input and the
   mainnet-preparation binding review. That holds over 500 genesis accounts.
   The other configuration inputs keep 65,536 bytes. The root genesis bundle
   signs the application genesis and the configuration together, so the root
   `max_request_bytes` input must cover both.
2. **Governance thresholds** are from 1 through 10,000 basis points each.
3. **Fee caps.** Each fee profile's `max_fee_cap` covers
   `max_transaction_gas × gas_price`; for the ordinary profile under
   governance, at the governance bound's highest gas price.
4. **Genesis within bounds.** The genesis gas price, creation fee and every
   governed cost lie within `parameter_bounds`, as do the lifecycle
   `min_self_bond` and `max_active`. The `max_active` bound is at most the
   reward state's `max_validators`.
5. **Transport.** `ordinary.max_transport_bytes` is at least the transport
   JSON (43 bytes) plus the base64 size of `limits.max_wire_bytes`.
6. **Controls.** Each root control bound holds its threshold's hex
   signatures (for emergency, the larger of the freeze and resume
   thresholds).
7. **Evidence seconds.** The lifecycle evidence age plus margin, in seconds,
   fits a signed nanosecond duration.

## Not changed

- The application's size limits are not compared with the engine's: a
  proposal already uses the smaller of the engine's block bytes and the
  application's `max_block_bytes`.
- The state-sync trust period is checked against the evidence age, which is
  below the unbonding horizon, so the documented rule holds.
- Genesis recovery accounts may carry recovery timings other than the
  account template's, which applies to accounts created later.
- The top-level `gas_price` field remains a pinned consistency input.
