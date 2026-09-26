# Adaptive issuance timing contract

Status: approved rules implemented for development integration. Production activation remains disabled. Production numeric parameters remain unselected.

## Approved timing rules

Each epoch has an explicit number of finalized blocks. The first epoch uses the command supplied in genesis. Later epochs use the controller command derived from the completed preceding epoch. No production epoch length or initial budget is inferred.

Each epoch budget splits into 40% validator rewards, 30% staking rewards, and 30% treasury funds. Each share uses integer floor division. A separate issuance reserve records the split remainder.

Each pool and the issuance reserve distribute evenly across the epoch's blocks. The first blocks receive any extra base units. The block schedule must sum exactly to each epoch allocation. Issuance reserve and staking reward rounding reserve remain separate records.

Controller transitions, issuance, staking rewards, supply totals, and block settlement must commit atomically. Development processing rejects a missing or invalid boundary observation. It does not fabricate observations, skip an epoch, or issue catch-up funds.

These rules do not make development block processing finalized production consensus. Production finality, authenticated observations, and activation approval remain separate requirements.

## Explicit genesis input

Fresh native genesis can supply `adaptive_issuance`. The object requires `reward_v2`. Every account must therefore meet the reward genesis vesting requirements.

| Field | Required input |
|---|---|
| version | 1 |
| profile | development |
| decimals | 6 |
| epoch_blocks | Explicit positive block count |
| initial_epoch_budget_udrt | Explicit integer digit string within controller bounds |
| controller | Complete controller inputs |
| max_recorded_epochs | Explicit positive history bound, at most 1,000,000 |

Controller inputs are target_ppm, shock_threshold_ppm, volatility_threshold_ppm, window_samples, integral_min, integral_max, soft, hard, base_udrt, min_udrt, and max_udrt. Both gain objects require proportional, integral, and derivative values. Dimensionless observations use parts per million. Controller bounds and arithmetic validation apply before any write.

No field receives an economic default. Missing fields, unknown fields, null, malformed budget strings, unsupported versions or scales, invalid limits, and a production profile reject.

The initial epoch budget is an issuance command. Genesis records it without issuing the epoch's funds immediately. Existing explicit genesis balances remain a separate supply input.

## Identity and atomic initialization

Timing identity includes chain ID, the monetary genesis digest, and the timing configuration. The monetary digest covers the exact source bytes. Input cannot supply an unrelated journal binding.

The importer holds the storage execution lock through initialization. It prepares the adaptive journal checkpoint, checks the expected absent predecessor, and appends that preparation to the same synchronous database batch as monetary accounts, supply counters, reward state, timing state, and the genesis marker. Preparation alone does not write or mint funds.

## Reopen and migration

Reopen requires the same monetary source marker and immutable timing configuration. The timing state, adaptive checkpoint, and recorded epoch history must agree. The importer checks the bounded adaptive and issuance namespaces, chain identity, monetary marker, and committed emission height. Full supply recovery checks remain part of the block settlement recovery path.

Reopen retains evolved epoch state. It does not restore the initial budget over committed history. Missing or corrupt timing state, a missing journal checkpoint, orphaned records, unknown namespaces, and configuration or binding mismatches reject without writes.

Missing adaptive_issuance preserves legacy development behavior only when no adaptive or issuance records exist. Existing state cannot gain timing activation through a changed genesis source. No automatic migration or journal replacement exists.

## Development fixtures and remaining evidence

Genesis tests use a synthetic three-block epoch and a 1,000 udrt initial budget. The controller fixture has a 500–2,000 udrt command range. Those values test validation and storage behavior. They are not production parameter selections.

The tests cover explicit required inputs, rejected malformed settings, combined initialization, exact reopen, and rejected legacy or orphaned state. Lifecycle tests must additionally verify boundary observations, the per-block pool schedule, atomic settlement, supply reconciliation, and recovery after faults.

Production epoch length, controller calibration, initial epoch command, beneficiary records, observations, finality, and release qualification remain required. This implementation does not authorize mainnet activation.
