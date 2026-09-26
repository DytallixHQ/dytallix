# E04 ordinary-v3 fee and nonce settlement map

Status: implementation map only. No ordinary-v3 fee or nonce settlement is active.

`src/` paths start at `dytallix-fast-launch/node`. `crates/` paths start at `dytallix-node`.

## Verified source boundary

- `src/governance_signed_admission.rs::assess_signed` checks the v3 signature, exact candidate fee profile, account domain, active key, authorization generation, spending nonce, expiry, protection state, and one governance action. It returns no fee reservation or settlement authority. It checks the committed parent supplied by the caller, but cannot prove that parent's provenance.
- `crates/protocol-types/src/ordinary_fees_v3.rs::FeeProfileV3` binds contract version 2, the v3 profile digest, activation height, UDRT fees, signed gas limit, fee cap, and governance action costs for tags 13 through 15. Its `base` field supplies common prices and limits. The v3 digest is different from the v2 base-profile digest.
- `src/ordinary_meter.rs::OrdinaryMeter` accepts a v2 body. Its action table has 12 entries. `SharedBlockMeter` now also binds one v3 profile whose base digest equals the v2 block profile. `src/governance_v3_meter.rs` charges v3 wire bytes, one signature, metadata, governance action tags 13 through 15, and supplied logical state records against that same block counter. No consensus caller yet proves those records came from the verified staged state.
- `src/ordinary_logical.rs` now has a bounded `governance:v1:state` logical record built from canonical `GovernanceState::encode` bytes. It has no separate ballot, deposit-stage, or escrow record. The adapter is not connected to consensus.
- `src/ordinary_fee_settlement.rs::plan_fee_accounting` and `src/ordinary_execution.rs::execute` accept v2 verified transactions, v2 receipts, and a v2 fee history. The retained receipt check and recovery verifier in `src/consensus_settlement.rs` also identify v2 input. These cannot record v3 acceptance unchanged.

## Required implementation sequence

1. Define a v3 fee-meter entry point from a verified `ordinary_v3::SignedOrdinary`. Measure the canonical v3 wire length with `ordinary_v3::encode`. Use the explicit v3 profile digest for the block context. Price exactly one signature and governance tag 13, 14, or 15. Count validation reads, action reads, proposed writes, and receipt metadata with the established terminal rejection and out-of-gas rules. Do not accept a caller-supplied `gas_used` as proof of work.
2. Define bounded logical records and independent byte vectors for governance state and each native account or custody record that a proposal, deposit, or vote can read or write. Record reads before acceptance and writes after acceptance. Retain measured work when an action fails or exhausts gas.
3. Derive the reservation from the verified v3 transaction ID and exact profile digest. Reserve the full signed UDRT fee cap from current eligible account liquidity. For `GovernanceDeposit`, reserve the signed UDGT amount from eligible, unrestricted account liquidity in addition to the UDRT fee cap. Proposal and vote have no signed UDGT debit. Check the signed cap against `gas_limit * gas_price` and the profile maximum. Recheck funding against the staged parent before execution.
4. Recheck signature, active authority, nonce mirrors, profile, expiry, protection, and governance action state against the staged parent immediately before fee acceptance. Preserve the signed v3 body and its canonical transaction ID. Do not reinterpret it as ordinary-v2.
5. After fee acceptance, settle `charge = max(gas_used, minimum_gas) * gas_price`. Require `charge <= maximum_fee`; set `released_cap = maximum_fee - charge`. Debit the account UDRT balance and eligible amount by `charge`, and increase withheld UDRT by the same amount. The signed DGT deposit moves to custody only if its action succeeds. On an accepted action failure or out-of-gas result, charge measured work and advance the nonce, but discard all proposed governance and DGT action effects.
6. Advance `RecoveryBook.accounts[actor].recovery.spending_nonce` and the mapped native account nonce from the same prior value to the same checked next value. Keep both v2 and v3 transactions in one nonce sequence. Reject stale or future nonce values at settlement.
7. Retain a versioned v3 receipt and its exact v3 profile. Bind the receipt to the transaction ID, envelope hash, actor, block position, profile, signed cap, gas counters, fee conservation, action outcome, and nonce transition. Reject an accepted transaction ID replay. An exact retained retry can return the existing receipt only for the same block position and envelope hash; it must not apply a second fee or nonce change. Enforce unique block positions across v2, v3, and sponsored recovery receipts.
8. Add the v3 input/receipt pair and its profile to the combined history validator. Recompute the signed v3 identity and verify every retained fee, nonce, result, and position. Commit the governance transition, native balances, UDRT withholding, both nonce mirrors, retained receipt/profile, ordered result, and block head in one atomic batch only after the reviewed adapter and current E04 activation decision exist.

## Required focused evidence before integration

- Valid proposal, deposit, and vote each use the exact v3 profile and governance action price.
- Tampered signature, stale key/generation/nonce, wrong domain/profile, expired request, protected account, and unsupported action reject without a fee or nonce change.
- Insufficient UDRT or DGT liquidity rejects before acceptance. Deposit funding includes its full signed DGT amount and the full UDRT fee cap.
- Accepted success, application failure, and out-of-gas cases conserve UDRT and advance both nonce mirrors once. Failed actions preserve no governance or DGT custody effect.
- Duplicate transaction ID, altered envelope at one ID, duplicate block position, and mixed v2/v3 nonce replay reject. Recovery validation detects missing or changed retained v3 evidence.

The exact governance state transitions and action-class approvals remain pending in `src/runtime/governance_candidate.rs::validate_for_activation`. This map does not change the `NO GO` gate.

## Local evidence, 2026-09-25

`cargo test --manifest-path node/Cargo.toml --lib --features pqc-fips204 governance_signed_admission -- --nocapture` passed 4 admission tests. It did not test v3 fee funding, gas settlement, nonce writes, retained receipts, or consensus integration. No Rust module or consensus file was changed for this map.

Later shared-meter implementation: `cargo test -p dytallix-fast-node --lib governance_v3_meter --no-default-features --features pqc-consensus --quiet` passed 8 tests. A focused test charges ordinary-v2 wire rejection, recovery work, and a v3 governance transaction to one counter. A separate test checks logical read and write costs. Binding those records to verified staged state, fee settlement, nonce writes, retained receipts, and consensus dispatch remain open.
