# Adaptive emission batch: 9 September 2026

The user selected the corrected whitepaper adaptive emission model for mainnet.
The selection is recorded as D01. Scheduled issuance remains development behavior.

## Delivered

- A separate dependency-free Rust controller, with integer arithmetic and versioned
  logical state. The node does not call this controller or mint from its output.
- A corrected transition specification for error history, regime boundaries,
  gain damping, integral projection, command bounds, rounding, and rejection.
- A conditional stability derivation for both gain regimes and the full finite
  window. It covers clipping, damping, and the specified integer arithmetic.
- An exact rational certificate checker and 13 independent reference vectors.
- Six controller tests, passing in debug and release builds. The same six cases
  in two build modes are not twelve independent tests.
- A WebAssembly compile check, strict controller lint checks, and a separate
  continuous integration job. Remote continuous integration has not run.

## Legacy integration tests

Test setup now shares the staking object that governance retains. It no longer
tries to obtain exclusive ownership of an object with two owners.
The default-gas assertions now match the existing development default of 2,000.
Reward tests now use delegation to update both an individual stake and total stake.
An emission update distributes previously pending rewards in the zero-stake test.
These changes do not change runtime governance, fees, staking, or emission rules.

The selected-node suite now reports 212 passed, 8 failed, and 0 ignored.
The previous recorded suite reported 203 passed and 17 failed.

| Remaining failure group | Count | Observed cause or limit |
|---|---:|---|
| Stake-weighted governance | 5 | Vote enumeration uses cached account addresses. Stake-only fixture voters are absent from that cache. The event fixture also has only 50% participation against the 67% default quorum. Liquid-balance fallback conflicts with the zero-stake voting expectation. |
| Staking reward accrual | 1 | The test expects 250,000 uDRT immediately. The scaled reward index yields 249,999. Residual ownership and settlement require a defined conservation rule. |
| Mempool performance | 2 | Debug timing thresholds still fail. Define the intended workload and optimized benchmark before changing acceptance limits. |

No expected vote weight or reward amount was weakened to hide these failures.
The full workspace previously had stale core APIs that prevented compilation.
This batch did not change or recheck those core targets. Their earlier failure
record remains open. Node test counts do not measure mainnet completion.

## Next work

1. Obtain calibration data for the utilization response, emission range, price
   response, delay, and uncertainty. Test whether the base emission supports the
   target equilibrium. Development traffic alone does not establish market demand.
2. Select candidate gains and a window from that evidence. Check the sufficient
   stability bound. If it is too conservative, complete a stronger proof before
   accepting gains outside its domain.
3. Specify epoch observation aggregation, missing data, and oracle validity.
   Resolve D02–D05 before implementing allocation, timing, fees, and burn settlement.
4. Implement canonical state encoding, configuration migration, and atomic
   controller/issuance/supply transitions. Define the first epoch's issuance.
5. Close the governance, reward-conservation, benchmark, and core compilation
   failures. Complete distributed consensus and the stake lifecycle under D06–D07.
6. Obtain independent mathematical, cryptographic, and security review. Qualify
   a frozen candidate with independent validators and recovery exercises.

The controller proof is conditional on a stated plant model. No measured mainnet
gain table, mainnet activation, production deployment, or launch approval follows
from this batch. The existing sealed security scan is unchanged.
