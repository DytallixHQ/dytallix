# Recovery CLI

`dytallix recovery` builds account recovery transactions with the
protocol's own codec. It covers all nine actions: `enroll`, `rotate`,
`start`, `finalize`, `cancel`, `resume`, `stage_policy`, `activate_policy`
and `cancel_policy`. The parties who must sign, such as guardians, a new key
and the active key, each sign on their own machine. A **separate sponsor
account pays** the fee; an account never pays for its own recovery.

It selects no endpoint, account, key or fee limit for you.

## 1. Capture the views

```sh
dytallix recovery query --endpoint http://127.0.0.1:26657 --account-id TARGET_64_HEX --output target-view.json
dytallix recovery query --endpoint http://127.0.0.1:26657 --account-id SPONSOR_64_HEX --output sponsor-view.json
```

A view is the node's `/recovery/account/{id}` report. It holds:
- the account's status and active key;
- its generation and nonces, the sponsor nonce included;
- its guardian policy and sequences;
- any pending recovery or staged policy;
- the timing rules and the fee profile.

It is not a consensus proof. Trust the endpoint through your deployment
procedure.

## 2. Prepare, offline

Write the action as JSON. Keys are `{"algorithm":"mldsa65","public_key":[...]}`:

```json
{"action":"start","replacement":{"algorithm":"mldsa65","public_key":[...]}}
```

```sh
dytallix recovery prepare --view target-view.json --request request.json --expiry-height H --output operation.json
```

- Counters and pending IDs come from the view.
- `start` and `stage_policy` take an optional `request_id` or `update_id`
  (64 hex). A random one is chosen otherwise.
- The expiry must be above the view's height and within its submission
  lifetime.
- The operation file lists who must sign:
  - `operation_keys` must all sign in the operation role;
  - at least `guardian_threshold` of the `guardians` must also sign in the
    operation role;
  - every key in `possession_keys` must sign in the possession role.

## 3. Each party signs, offline

```sh
dytallix recovery sign --operation operation.json --role operation --wallet NAME --output guardian-1.sig.json
dytallix recovery sign --operation operation.json --role possession --key-file new-key.json --output new-key.sig.json
```

A key the operation does not name for that role is refused. `--wallet` reads
the encrypted keystore and asks for its passphrase. `--key-file` reads an
owner-only file with `algorithm`, `public_key` and `private_key`.

## 4. Assemble, offline

```sh
dytallix recovery assemble --operation operation.json --signature guardian-1.sig.json \
  --signature guardian-2.sig.json --signature new-key.sig.json --output signed.json
```

It verifies each signature, drops duplicates, checks the requirements and
orders the signatures as the wire requires.

## 5. Sponsor, offline

```sh
dytallix recovery sponsor --signed signed.json --target-view target-view.json --sponsor-view sponsor-view.json \
  --wallet SPONSOR --gas-limit G --maximum-charge-udrt C --expiry-height H --output transaction.json
```

- The sponsor's key must be its account's active key, and that account must
  be in normal status.
- The gas limit must lie within the fee profile's bounds.
- The maximum charge must be at least the gas limit times the gas price, and
  at most the profile's cap.

## 6. Submit and check the receipt

```sh
dytallix recovery submit --endpoint http://127.0.0.1:26657 --transaction transaction.json
dytallix recovery receipt --endpoint http://127.0.0.1:26657 --transaction transaction.json
```

- CheckTx acceptance is not commitment.
- `receipt` reads the sponsor receipt and verifies it against the pinned
  chain's application hash. An absent receipt is not success.
- Receipts are kept until the operation's submission expiry.

## The key compromise path

| Step | Action | Who signs |
| --- | --- | --- |
| 1 | `start` to a new key | Two guardians (operation) and the new key (possession) |
| 2 | Wait for the recovery delay | — |
| 3 | `finalize` | The new key (operation) |

- `rotate` also replaces the key, but the active key signs it, so an
  attacker holding that key can race it.
- `cancel` locks the account, and `resume` unlocks it with the same key.
- Only one recovery can be pending at a time.
