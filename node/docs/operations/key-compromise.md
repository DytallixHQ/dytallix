# Key compromise

Runbook v1 ([index](README.md)). The roles, custody records and channel are
unset (D14-Q03, E05, P02). Treat any key that has left its approved custody
as compromised, including every leaked testnet key. A testnet key is never
a mainnet key.

| Key | Where | What an attacker can do | Response |
| --- | --- | --- | --- |
| Validator consensus key (ML-DSA-65) | `config/priv_validator_key.json` | Sign votes as the validator, including conflicting ones | Stop, fence, rotate or exit |
| Validator operator account key | The operator's wallet | Validator actions (exit, bond, unbond), transfers | Guardian recovery of the account |
| Node peer key (ML-DSA-65) | `config/node_key.json` or `config/pqc_peer_seed.bin` | Connect to peers as this node | Replace it and update every peer's pins |
| Channel endpoint key (ML-DSA-65) | `config/client_channel_seed.bin` | Answer as this node's public endpoint to clients that pin it | Replace it and publish a new pin |
| User account key | The user's wallet | Spend and act as the account | Guardian recovery; freeze for a wide compromise |
| Root authority keys (SLH-DSA) | Custody | With enough keys: emergency, upgrade and handover controls | No replacement mechanism yet; a kit replacement control, signed by three kits, is approved to be built before launch (P01, 7 October 2026) |
| CLI keystore | `~/.dytallix/keystore.json` | Version 2: the keys, if the passphrase is also known or guessed. Version 1 (plaintext): every key in it | Handle each key it held; migrate any version 1 file |

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
3. **Rotate to a new key**, unless the validator has a recorded fault.
   1. On the validator host, generate the new key into a new directory:
      ```sh
      dytallix-validator-key generate --key-file NEW/priv_validator_key.json \
        --state-file NEW/priv_validator_state.json
      ```
   2. Read the operator account's spending nonce
      (`dytallix ordinary query-account`) and choose the last height at
      which the proof is valid.
   3. Sign the possession proof with the new key:
      ```sh
      dytallix-validator-key proof --key-file NEW/priv_validator_key.json \
        --genesis HOME/config/genesis.json --operation rotate --validator ID \
        --owner OPERATOR_ADDRESS --nonce N --expiry-height H
      ```
      - `--owner` is the operator account's address, as the lifecycle
        configuration's approved operators name it.
      - The nonce must be the one the transaction uses.
      - The tool signs only the proof it builds, for the engine genesis
        chain. It prints the `ValidatorRotateKey` action.
   4. Put that action in the actions file (`[ACTION]`) and submit it with
      the operator account, as below.
   5. The new key takes effect two blocks after the rotation commits. Then:
      - move the old key and its state aside (keep them as evidence);
      - install the new key and its fresh state as `HOME/config/priv_validator_key.json`
        and `HOME/data/priv_validator_state.json`;
      - update the supervisor's validator pins (`validator_public_key_sha256`
        and the key file's engine input hash);
      - restart.

   A rotation is refused while the validator has a recorded fault
   (`VALIDATOR_EXPOSURE_BARRED`); exit instead. The old key can never be
   registered again (`CONSENSUS_KEY_ALREADY_USED`).
4. **Or remove the key's power.** The operator submits `ValidatorExit` with
   the operator account, as an ordinary v2 transaction:
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
   - Each owner can withdraw once the unbond matures and no penalty on it is
     unsettled ([penalties v1](../architecture/penalties-v1.md)).
5. **Penalty.** If the key double-signed, the validator's first duplicate
   vote deducts the penalty rate from every stake bonded to it at that
   height and removes the validator for good, two blocks after the evidence
   commits (D09-Q04). Light-client-attack evidence is recorded only.

## Validator operator account key

The operator account authorizes the validator's actions. An attacker could:
- exit the validator;
- unbond its stake to the attacker's control;
- send liquid funds.

The account's key is replaced through recovery, with `dytallix recovery`
([recovery CLI](../../../sdk/docs/recovery-cli.md)). Every transaction is
paid by a separate sponsor account.

1. **Start a guardian recovery to a new key.** Two guardians sign in the
   operation role and the new key in the possession role. Starting advances
   the account's generation, which voids anything the old key signed. The
   account's own actions are then blocked while the recovery is pending.
2. **Finalize after the recovery delay.** The new key signs.
3. **Race.** `rotate` needs only the active key, so an attacker holding it
   can rotate first. That is why the guardian path is the response. If a
   recovery was started against you, the guardians can `cancel` it, which
   locks the account; a new `start` recovers it.
4. **Meanwhile,** keep the validator's consensus key safe; it is not
   affected.

## Node peer key

1. **Generate a new peer key.** It must stay separate from the validator
   key.
2. **Update the pins.** Every peer replaces the old public key with the new
   one in `config/pqc_transport.json`. Peers are admitted only by their
   pinned full public key, so a peer that keeps the old pin still admits
   the attacker.
3. **Update the service pins.** Update the supervisor's engine input pins
   (the SHA-256 of each file) and restart in a coordinated order.

## Channel endpoint key

The key lets an attacker who can reach clients answer as the endpoint. It
cannot sign transactions or forge state proofs, since clients verify those
against the pinned chain. It can still misreport results that carry no
proof, such as status, CheckTx and broadcast results.

1. **Replace the seed.** `dytallix-channel-key generate` never replaces a
   file, so first move the old seed out of the service paths and keep it
   as evidence. Then generate the new seed and write its pin with
   `dytallix-channel-key pin`.
2. **Update the supervisor's pin.** Update `adapter_channel.pin` (the
   file's SHA-256) and restart. The readiness probe fails until the seed
   and the pin match.
3. **Replace the pin at every client.** Version 1 has no key overlap or
   revocation list. A client that keeps the old pin trusts the attacker.
   Tell users through a channel they already trust.

## User account keys

- **One account:** guardian recovery, as above. An account without an
  enrolled guardian policy can only `rotate`, which the attacker can race;
  enroll guardians before a compromise.
- **Many accounts,** for example a compromised wallet release: an emergency
  freeze stops every user transaction while consensus continues
  ([emergency transaction freeze](../mainnet/emergency-transaction-freeze.md)).
  It needs 3 of the 5 freeze keys. Resume has its own authority. Both are
  signed offline with three key kits
  ([control signing](../mainnet/control-signing.md)).

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
