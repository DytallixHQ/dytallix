# Root control signing

Engineering task E05. A root control is an emergency freeze or resume, an
upgrade admission, activation or cancellation, a release handover
admission, activation or cancellation, or a kit replacement. The node admits
one as a transaction carrying three signatures from the five keys of its
authority
([emergency freeze](emergency-transaction-freeze.md),
[upgrade execution](upgrade-execution.md),
[root kit replacement](../architecture/root-kit-replacement-v1.md)). This
document is how such a control is made in production
([control signing approval](../../../launch/approvals/P01_E05_CONTROL_SIGNING_2026-10-06.json)).

The keys that sign are those of the root authority epoch in force: the
configuration's until a kit replacement takes effect, then the epoch's that
the node's status reports. Prepare and assemble read them from the status.

## The steps

1. **Prepare, online.** Save the node's `/status` view and run
   `dytallix-control prepare`. It writes a signing request.
2. **Sign, offline.** Three holders each sign on their own ceremony
   machine ([key ceremony](../../../launch/custody/KEY_CEREMONY.md)) with
   their kit: `dytallix-root-sign sign-control` signs the request and writes
   one signature file per key. The request and the signature files are
   public, so they travel by any channel; the kits never leave their
   holders.
3. **Assemble, online.** Save the status again and run `dytallix-control
   assemble`, which turns the request and the three signature files into the
   control.
4. **Check and submit.** The CLI dry-runs the control against the node
   (`check_tx`) and submits it (`broadcast_tx_sync`) over the pinned chain.

`dytallix-control` (prepare and assemble) is in the node crate and ships in
the release; `dytallix-root-sign` is the offline signer; `dytallix control`
is the CLI. The node's tests `control_tools_freeze_and_resume_end_to_end`
(five kits, a freeze signed by kits 1, 3 and 5, a resume by kits 2, 3 and 4)
and `control_tools_replace_a_kit_end_to_end` (seat 5 replaced, then a freeze
signed with the new kit) run the whole chain against the application.

```text
dytallix control status --out status.json                    # online
dytallix-control prepare freeze --config application-config.json \
  --status status.json --incident incident.md --out request.json
dytallix-root-sign show-control -request request.json        # offline, per kit
dytallix-root-sign sign-control -request request.json \
  -private-key /mnt/kit/kit-N-freeze.key -public-key /mnt/public/kit-N-freeze.json \
  -operation freeze -sequence S -out kit-N.sig.json
dytallix control status --out status-2.json                  # online
dytallix-control assemble --config application-config.json --status status-2.json \
  --request request.json --out control.json kit-*.sig.json
dytallix control check control.json
dytallix control submit control.json
```

## Prepare

```text
dytallix-control prepare OPERATION --config application-config.json \
  --status status.json --out REQUEST.json [--window-blocks N] [inputs]
```

| Operation | Inputs |
| --- | --- |
| `freeze` | `--incident FILE` (or `--incident-sha256`) |
| `resume` | `--incident` (the same document as the freeze), `--readiness FILE` |
| `upgrade-admit` | `--authorization FILE` |
| `upgrade-activate` | `--evidence FILE` |
| `upgrade-cancel` | none |
| `handover-admit` | `--target-release-sha512 HEX`, `--transition receipt-index-v1` or `schema-preserving`, `--authorization FILE` |
| `handover-activate` | `--evidence FILE`; for the receipt index transition, `--upgrade-activation CONTROL`, the assembled upgrade activation |
| `handover-cancel` | none |
| `kit-replacement` | `--seat N`, `--old-kit DIR`, `--new-kit DIR`: see [Kit replacement](#kit-replacement) |

Every other field comes from the configuration (chain, genesis, policy
digest, bounds) or the status:

- **Anchor.** The status height and app hash. The block after it is the
  window's first height.
- **Authority.** The epoch in force at the next block and its keys.
- **Window.** By default as long as the policy allows: its validity length
  and anchor age, 34,560 blocks (two days) each. The control must be signed,
  assembled and submitted before it closes. `--window-blocks` shortens it.
  While a kit replacement is pending, the window ends the block before its
  effect height: the keys that sign change there. Prepare after it for a
  control the new keys sign.
- **Sequence.** The family's next sequence.
- **Release.** The active release.
- **Pending plan.** For an activation or cancellation, the admitted plan and
  its admission receipt. The latest emergency receipt is bound into every
  activation, so a freeze or resume after preparing one means preparing it
  again.
- **Resume.** It binds the freeze receipt, which is the latest emergency
  receipt while frozen, and the anchor as the restored state.

An activation is refused before its notice period ends. A handover with the
receipt index transition activates together with the upgrade activation:
sign and assemble the upgrade activation first, then prepare the handover
activation with it, and submit both.

## The request

`dytallix.control-request.v1`, JSON:

| Field | Meaning |
| --- | --- |
| `operation`, `kind` | What it is, and the node's control kind |
| `anchor_height`, `anchor_app_hash` | The finalized block it is anchored to |
| `payload` | The node's payload for the kind |
| `artifact_hex` | What the signatures cover: the kind's domain followed by the payload's canonical bytes |
| `envelope` | `chain_id`, `action` (`emergency` for freeze and resume, `upgrade` otherwise), `sequence`, `not_before_height`, `not_after_height` and `artifact_sha512` |
| `authority` | The purpose (`freeze`, `resume` or `upgrade`), threshold, maximum signatures and keys, for the signer's information; a kit replacement also lists the new keys |

Each custodian signs the root envelope (root-authorization `Envelope`):
version 1, SLH-DSA-SHAKE-256s, the chain, action, sequence and window, and
the artifact's SHA-512, under the FIPS 205 context `DYTALLIX/ROOT/v1/` plus
the action. The node verifies each signature through its pinned root helper.
The signer recomputes the digest from `artifact_hex` and reads the summary it
shows from the artifact itself, never from the request's other fields.

## The signature

`dytallix.control-signature.v1`, JSON: `key_id` (the SHA-256 of the public
key), `artifact_sha512`, `sequence` and `signature_hex` (29,792 bytes).

## Assemble

```text
dytallix-control assemble --config application-config.json --status STATUS.json \
  --request REQUEST.json --out CONTROL.json SIGNATURE.json...
```

It re-encodes the payload through the node's types and refuses one that is
not exactly the signed artifact. It counts only keys of the authority in
force for the control's purpose (from the configuration and the status,
never the request's copy), and refuses a request that names another epoch
than the one in force: prepare it again. It refuses a signature for another
artifact or sequence and a key that signed twice, and needs at least the
threshold and at most the policy's maximum (three in production). It sorts
the signatures by key ID and checks the result with the node's own control
decoder. It does not verify the signatures; the node's dry run does.

## Kit replacement

One control replaces one seat's upgrade, freeze and resume keys
([root kit replacement v1](../architecture/root-kit-replacement-v1.md)):
three current upgrade keys sign it, and the new kit's three keys sign the
same request as their proofs of possession. It takes effect 120,960 blocks
(seven days) after it is admitted. No replacement is admitted while the
chain is frozen or while another is pending.

1. **The new kit.** The new holder makes a kit for the seat number in the
   [key ceremony](../../../launch/custody/KEY_CEREMONY.md):
   `dytallix-root-sign kit -number N`, onto their new fob, with the public
   key records in a directory of their own. The kit's genesis key is not
   used.
2. **Prepare.** With the leaving kit's public key records (from the custody
   records) and the new kit's:

   ```text
   dytallix-control prepare kit-replacement --config application-config.json \
     --status status.json --seat N --old-kit OLD-PUBLIC-DIR --new-kit NEW-PUBLIC-DIR \
     --out request.json
   ```

3. **Show.** `dytallix-root-sign show-control` shows the leaving and the new
   key IDs per role. Every signer checks them against the custody records
   and the new kit's records before signing.
4. **Sign.** Three current holders sign with their upgrade keys
   (`-operation kit-replacement`). The new holder signs three times, with
   the new kit's upgrade, freeze and resume keys.
5. **Assemble, check and submit** as for any control: six signature files.
   The control is about 240 KB.
6. **After the effect height** the status reports the new epoch and its
   keys, and controls are prepared for it. The leaving kit no longer signs.

Outputs are never overwritten.
