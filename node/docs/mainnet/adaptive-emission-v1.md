# Adaptive emission reference, version 1

Status: implementation candidate. No activation or mainnet parameter approval.
The user selected the corrected whitepaper adaptive model on 9 September 2026.
This specification implements that model as a separate policy module.
It does not mint tokens. The node still uses its development schedules.

## Source and corrections

Source: supplied Dytallix Tokenomics Paper, physical pages 5–8.
The earlier mathematical register records the paper defects as TM-01–TM-07.

| Paper ambiguity | Candidate correction |
|---|---|
| Both regimes use the same inequality | Soft when absolute error is below delta; hard otherwise. Equality selects hard. |
| W+1 terms called a W-sample window | W means sample count. Include the current sample and at most W−1 prior samples. |
| 1,008 epochs called one week | Specify sample count separately from duration. At 200 seconds, a week has 3,024 epochs. D03 remains open. |
| Derivative damping differs between prose and code | Apply volatility damping to all three gains. |
| Startup and update order absent | Empty initial history; previous error zero; insert current error before summation. |
| Floating-point reference | Integer arithmetic, with explicit units and rounding. |
| Unqualified stability guarantee | Use the conditional theorem in adaptive-emission-proof.md. Calibrate its plant assumptions separately. |

These are explicit engineering corrections for review. They do not approve the
allocation, timebase, oracle rules, fee rules, or parameter values for genesis.
The finite window remains a finite window. It is not an accumulating integrator.

## Units and configuration

Use S = 1,000,000 parts per unit for utilization and external volatility.
Utilization and its target are integers from 0 through S.
Volatility is a nonnegative u64 integer in the same dimensionless scale.
The controller accepts an observation only after the future consensus input
contract supplies it. A local network request is not an accepted input source.

Emission amounts use integer uDRT per epoch. One uDRT is one millionth of DRT.
Each gain is a nonnegative u64 number of uDRT per unit of dimensionless error.
Integral gains apply per sample; changing the epoch duration changes the model.
No configuration defaults constitute mainnet parameters.

Require 1 <= W <= 65,536, 0 < delta <= S, and
0 <= E_min <= E_base <= E_max <= u64::MAX.
Integral limits use signed sums in parts per million and satisfy
−W*S <= I_min <= 0 <= I_max <= W*S.
Soft and hard each specify all three gains. A larger hard proportional gain is
permitted but does not itself establish a stable domain.

## Transition order

Process input epoch t, starting at zero. Require consecutive epochs.
Return a command for epoch t+1. Reject duplicate or skipped epochs.
Reject out-of-range utilization and an epoch whose successor cannot be represented.
Rejection leaves the controller state unchanged.

1. Calculate e_t = target_ppm − utilization_ppm.
2. Read the previous error. Use zero before the first observation.
3. Remove the oldest error if the stored window is full. Insert e_t.
4. Sum the stored errors. Clamp the sum to [I_min, I_max], obtaining I_t.
5. Calculate d_t = e_t − e_(t−1).
6. Select soft gains when |e_t| < delta. Otherwise select hard gains.
7. If sigma_ppm exceeds its threshold, replace each gain g with
   floor(g*S/(S+sigma_ppm)). At equality, keep the selected gains unchanged.
8. Calculate raw = E_base + trunc(gp*e_t/S) + trunc(gi*I_t/S)
   + trunc(gd*d_t/S). Truncate each signed term toward zero.
9. Clamp raw to [E_min, E_max]. Return the command and diagnostic values.
10. Commit the error window and last input epoch to controller state.

The integral clamp does not overwrite the stored errors. Thus a sample leaves
the sum after W observations even if the sum was previously clipped.
The controller does not carry fractional uDRT from a term into later epochs.
Rounding defines the proposed command; it does not create an accounting balance.

## State and module boundary

`dytallix-adaptive-emission` has no external dependencies, storage, network,
clock, random source, unsafe code, or token balances.
Its logical checkpoint contains a version, configuration, last epoch, and errors
in oldest-first order. Restore validates the version, history length, epoch,
configuration, and feasible error range.

The canonical encoding is now specified in adaptive-state-format-v1.md.
It does not authenticate the source of state or prove that history came from
finalized blocks. The storage command journal persists checkpoints and commands
atomically. It does not mint, allocate, or settle supply.
The integration layer must verify a genesis-approved configuration and commit
the controller state, issuance, supply accounting, and epoch record atomically.
Configuration changes require an explicit migration. Do not edit history under
a new target or window without that migration.

## Evidence and activation requirements

The independent Python Fraction reference generates 13 sequential vectors.
Vectors cover startup, both sides of the regime boundary, threshold equality,
volatility damping, signed rounding, integral clamps, and window rollover.
Rust tests also cover restart equivalence, invalid configuration, state rejection,
epoch overflow, maximum arithmetic inputs, and output saturation.
These tests establish reference behavior. They do not establish market behavior.

Before integration, define epoch utilization aggregation, missing-block treatment,
observation freshness, oracle quorum, initial epoch issuance, configuration
encoding, and governance migration. D02–D07 remain separate decisions.
Before activation, calibrate the plant, select gains within a proved domain,
integrate atomic supply accounting, and complete independent review and release
qualification. A successful reference test does not authorize activation.
