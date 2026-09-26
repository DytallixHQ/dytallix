# Ordinary logical records, version 1

These records measure typed state for ordinary fee accounting. They are not database records or authority proofs. The caller supplies a positive byte bound from its explicit local profile. No production bound is selected here.

The outer format is `LogicalRecord` version 1 in `src/ordinary_meter.rs`. Outer field integers are big-endian. Bytes payloads use fixed-width, little-endian bincode encoding. Integer magnitude does not change its encoded width. Strings, sequences and maps use eight-byte lengths inside those payloads. `usize` uses eight bytes. BTreeMap and BTreeSet use their sorted iteration order. The referenced state types contain no HashMap or HashSet.

| Logical key | Ordered fields |
|---|---|
| `acct:account:{address}` | 1 UTF-8 address; 2 u128 udgt; 3 u128 udrt; 4 u64 native nonce. Both balances are always present; a missing map entry encodes zero. Unknown denominations reject, including zero entries. |
| `recovery:account:{hex-id}` | 1 bytes: RecoveryAccount fields in declaration order, including RecoveryState and sponsor nonce. The ID must equal the state's account ID. |
| `ordinary:grant:{hex-id}` | 1 u8 presence. If present: 2 owner digest, 3 beneficiary digest, 4 u64 owner generation, 5 u64 period, 6 u64 last-active height. Only grant version 1 is permitted; owner must match the key. Absence has field 1 only. |
| `rewards:v2:state` | 1 bytes: RewardState in declaration order. |
| `lifecycle:v1:state` | 1 bytes: explicit LifecycleLogical view in `src/ordinary_logical.rs`. |
| `penalty:v1:state` | 1 bytes: PenaltyState in declaration order. |
| `emission:pool:staking_rewards` | 1 u128 amount. |

The native balance fields have fixed size before fee reservation, during action execution and after unused-cap release. Removing or restoring a zero-valued physical balance entry cannot change the logical record size. This replaces the unpublished balances-map candidate before qualification.

LifecycleConfig has a decimal-text serializer for `min_self_bond`. The logical view excludes that display serializer and encodes the value as sixteen bytes. The same view applies to all unbond evidence configurations, including scheduled removals. Other lifecycle fields retain their typed declaration order. This encoding deliberately differs from physical lifecycle storage.

The adapter does not check module validity, cryptographic authority, recovery protection, funding or fee acceptance. Its caller must perform those checks. Serialization failure, ID mismatch, unsupported native denomination or grant version, or an exceeded logical bound returns an internal meter error.

`src/ordinary_logical_tests.rs` supplies independent byte vectors for each record family. It also checks exact bounds, grant absence, ID binding and fixed numeric widths in nested evidence snapshots. Changes to these layouts require updated version/profile compatibility and independent vectors before activation.
