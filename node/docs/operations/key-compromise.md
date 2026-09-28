# Key compromise

Runbook v1 ([index](README.md)). The roles, custody records and channel are
unset (D14-Q03, E05, P02). Treat any key that has left its approved custody
as compromised, including every leaked testnet key. A testnet key is never
a mainnet key.

| Key | Where | What an attacker can do | Response |
| --- | --- | --- | --- |
| Validator consensus key (ML-DSA-65) | `config/priv_validator_key.json` | Sign votes as the validator, including conflicting ones | Stop, fence, exit |
| Validator operator account key | The operator's wallet | Validator actions (exit, bond, unbond), transfers | Rotate or recover the account (blocked, gap 17) |
| Node peer key (ML-DSA-65) | `config/node_key.json` or `config/pqc_peer_seed.bin` | Connect to peers as this node | Replace it and update every peer's pins |
| User account key | The user's wallet | Spend and act as the account | Rotate or recover (blocked, gap 17); freeze for a wide compromise |
| Root authority keys (SLH-DSA) | Custody | With enough keys: emergency, upgrade and handover controls | No replacement mechanism |
| CLI keystore | `~/.dytallix/keystore.json` | Every key in it: stored in plaintext until gap 16 | Handle each key it held |

## Signals

- **Signatures while stopped.** The validator's address signs commits
  (`/block?height=H`, last commit) at heights where its own signer was
  stopped.
- **Faulty validators.** `dytallix_engine_consensus_byzantine_validators`
  is above zero, or the blocks record duplicate-vote evidence for the
  validator.
- **Unauthorized transactions.** Receipts show actions the operator did not
  sign: an unexpected `ValidatorExit`, `RewardBeginUnbond` or transfer.
- **Custody.** A custody or host record shows the key's material was
  exposed.

## Validator consensus key

1. **Stop and fence the validator.** Stop the supervisor and keep
   `data/priv_validator_state.json`. Stopping this node does not stop a
   copy of the key elsewhere.
2. **Assess the voting power at risk.** If the compromised validators hold
   a third of the power or more, the attacker can stop the chain. Above two
   thirds they can finalize conflicting blocks. Follow [halt.md](halt.md)
   and [fork.md](fork.md), case B.
3. **Remove the key's power.** The operator submits `ValidatorExit` with the
   operator account, as an ordinary v2 transaction:
   ```sh
   dytallix ordinary query-profile --endpoint URL --output profile.json
   dytallix ordinary query-account --endpoint URL --account-id OPERATOR --output account.json
   dytallix ordinary prepare --profile profile.json --account account.json --context context.json \
     --actions exit.json --memo "validator exit" --expiry-height H --gas-limit G \
     --maximum-fee-udrt F --output body.json
   dytallix ordinary sign ... && dytallix ordinary submit ... && dytallix ordinary receipt ...
   ```
   Here `exit.json` is `[{"type":"ValidatorExit","validator_id":"ID"}]`.
   - The validator leaves the set two blocks after the exit commits.
   - Every position bonded to it, the operator's and each delegator's,
     starts unbonding and matures after the evidence window.
   - Withdrawing the stake fails as `VALIDATOR_WITHDRAWAL_DISABLED` (a paid
     failure) while the penalty profile is off.
4. **Rotation is blocked.** `ValidatorRotateKey` needs a possession proof
   signed by the new key, and there is no production proof signer (gap 17).
   It is also refused while the validator has a recorded fault
   (`VALIDATOR_EXPOSURE_BARRED`). The old key can never be registered again
   (`CONSENSUS_KEY_ALREADY_USED`).
5. **No penalty.** Evidence is recorded, not penalized (D09-Q04). Removing
   the validator's power is the only response the chain offers.

## Validator operator account key

The operator account authorizes the validator's actions. An attacker could:
- exit the validator;
- unbond its stake to the attacker's control;
- send liquid funds.

1. **Race to rotate.** Replace the account's key before the attacker acts.
   Use a recovery `Rotate` signed by the active key, or guardian recovery:
   `Start`, then `Finalize` after the delay. `Cancel` locks a recovery in
   progress.
2. **Blocked:** no client builds recovery transactions yet (gap 17).
3. **Until then,** watch the account's receipts, and keep the validator's
   consensus key safe; it is not affected.

## Node peer key

1. **Generate a new peer key.** It must stay separate from the validator
   key.
2. **Update the pins.** Every peer replaces the old public key with the new
   one in `config/pqc_transport.json`. Peers are admitted only by their
   pinned full public key, so a peer that keeps the old pin still admits
   the attacker.
3. **Update the service pins.** Update the supervisor's engine input pins
   (the SHA-256 of each file) and restart in a coordinated order.

## User account keys

- **One account:** the user rotates or recovers as above (blocked, gap 17).
- **Many accounts,** for example a compromised wallet release: an emergency
  freeze stops every user transaction while consensus continues
  ([emergency transaction freeze](../mainnet/emergency-transaction-freeze.md)).
  It needs 3 of the 5 freeze keys. Resume has its own authority. Signing
  controls in production depends on custody (E05, P02).

## Root authority keys

- There is no on-chain replacement mechanism for root keys
  (`consensus/root-authorization/README.md`). Controls cannot replace keys
  or change authority.
- Below the threshold (3 of 5 for emergency controls), a compromise has no
  immediate on-chain effect but narrows the margin. At the threshold, the
  attacker holds that authority.
- Escalate to the named root custodians. Replacing a root key is an open
  decision; this procedure cannot resolve it.

## Do not

- Copy a validator key to a second host "to keep signing".
- Reuse a compromised key after rotation.
- Put key material, even partial, in the incident record.
