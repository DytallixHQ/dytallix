# Key ceremony

Engineering task E05. Five people, the founder and four others, hold the
five root key kits, one each: kit N holds key N of each role (genesis,
upgrade, freeze and resume), and any three kits can act
([root key holders](../approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json),
[trust model](../TRUST_MODEL.md)). Each holder makes their own kit in their
own session, on an offline machine, and passes on only public records and
proofs. No one, the founder included, sees another holder's kit secret.

How (P01, [key ceremony approval](../approvals/P01_E05_KEY_CEREMONY_2026-10-05.json),
5 October 2026, as changed by the root key holders approval, 7 October 2026):

- **Storage.** Each kit is one encrypted USB stick, the **kit fob**, and
  nothing else: no paper copy and no backup.
- **Machine.** A laptop booted from a Debian live USB with networking off;
  keys never touch its disk or a networked system.
- **Wallet.** Each holder's 20,000 DGT account key is on a separate **wallet
  fob**, so the kit fob is never plugged into a networked machine.
- **Places.** Each holder keeps their kit fob in a place of their own,
  apart from their wallet fob. No two kits are ever kept together.

## How a kit works

`dytallix-root-sign kit` makes a random 32-byte kit secret and derives the
kit's four keys from it, one per role (SLH-DSA-SHAKE-256s, FIPS 205 key
generation from seeds derived with SHAKE256). It writes the four private
keys and the secret, as one checked line in `kit-N-paper.txt`, to the kit
fob. The secret recreates all four keys, so the fob is the kit.

- **A lost or failed fob, or a forgotten passphrase,** loses that kit. The
  other four still reach the threshold of three. The kit is then replaced
  with the [kit replacement control](../../node/docs/architecture/root-kit-replacement-v1.md)
  (to be built before launch): the holder makes a new kit with proofs for
  the next authority epoch, three kits' upgrade keys sign the replacement,
  and seven days later the old keys stop verifying.
- **Two kits lost** leaves three: every action then needs all three, so
  replace a lost kit before anything else.
- **Three kits lost or exposed** is beyond recovery: the root keys cannot be
  replaced on chain without three kits
  ([key compromise](../../node/docs/operations/key-compromise.md)).

Check your fob once a year on the offline machine (step 4.4 below): USB
storage can fade when it sits unpowered for years.

## What each holder needs

- An x86_64 laptop that can boot from USB. Its own disk is never used.
- A **live USB**: a Debian 12 live image written to a stick, checked against
  Debian's published SHA512SUMS before you write it. It must have
  `cryptsetup`, `mkfs.ext4` and a text editor; check during the rehearsal,
  since nothing can be installed offline.
- A **tools USB**: the release's static Linux `dytallix-root-sign` and its
  `SHA256SUMS` ([release](../../release/README.md)). Before the first release,
  build it with `release/reproduce.sh` and compare with CI's checksums. Check
  the SHA-256 on your online machine and again on the ceremony machine.
- One **kit fob**, labeled with your kit number.
- One **public stick**, formatted on your online machine (FAT32 or exFAT).
  Only public records and proofs go on it.
- A **wallet fob** for your DGT account.
- Your kit number N (the founder assigns 1 to 5), your **controller ID**,
  which names your kit and never you (for example `kit-holder-3`), and a
  **session ID** for your session (for example `ceremony-2026-11-02-kit-3`).

## The ceremony (each holder, for their own kit)

1. **Offline.** Unplug the network cable and turn off Wi-Fi (in the firmware
   settings if you can). Boot the live USB. Never connect to a network until
   you shut down.
2. **Tools.** Mount the tools stick, check the binary against `SHA256SUMS`
   (`sha256sum -c`), and copy it to the RAM disk:
   `install -m 0755 dytallix-root-sign /tmp/`.
3. **Public stick.** Mount it at `/mnt/public` and make
   `/mnt/public/proofs`.
4. **Your kit, N:**
   1. Plug in the kit fob and find it with `lsblk`. Encrypt it with a
      passphrase you will remember: nothing else can open it.

      ```text
      sudo cryptsetup luksFormat --type luks2 /dev/sdX
      sudo cryptsetup open /dev/sdX kit
      sudo mkfs.ext4 -L kit-N /dev/mapper/kit
      sudo mount /dev/mapper/kit /mnt/kit && sudo chown "$USER" /mnt/kit
      ```

   2. Make the kit:

      ```text
      /tmp/dytallix-root-sign kit -number N -private-out /mnt/kit -public-out /mnt/public
      ```

      It writes the four private keys and `kit-N-paper.txt` to the kit fob
      and the four public key records to the public stick. It also prints
      the kit's line: do not write it down. The screen is cleared when you
      shut down.
   3. **Check the fob** against the public keys:

      ```text
      /tmp/dytallix-root-sign kit-check -paper /mnt/kit/kit-N-paper.txt -public /mnt/public
      ```

   4. **Prove possession** of each key. The epoch is 1 for upgrade, freeze
      and resume, the first key epoch, and `none` for genesis:

      ```text
      for p in genesis upgrade freeze resume; do
        e=1; [ "$p" = genesis ] && e=none
        /tmp/dytallix-root-sign prove -private-key /mnt/kit/kit-N-$p.key \
          -public-key /mnt/public/kit-N-$p.json -chain-id dytallix-mainnet-1 \
          -controller CONTROLLER -purpose $p -epoch $e -session SESSION \
          -out /mnt/public/proofs/kit-N-$p.json
        /tmp/dytallix-root-sign verify-proof -proof /mnt/public/proofs/kit-N-$p.json
      done
      ```

   5. Close the fob (`sudo umount /mnt/kit && sudo cryptsetup close kit`).
5. Unmount the public stick and shut down. The live system's memory is gone
   with the power.
6. **Store the kit fob** in your own place, never with your wallet fob and
   never with another holder's kit.
7. **Hand over the public stick** (or a copy of it) to the founder. It holds
   only public records and proofs.

## Your wallet fob

On your own online machine, with the release's CLI:

```text
dytallix wallet create --name kit-holder-N
dytallix wallet info
cp ~/.dytallix/keystore.json /Volumes/WALLET/keystore.json
```

`wallet create` encrypts the account key under your passphrase in the CLI's
keystore (version 2: AES-256-GCM under an Argon2id key). Copy that encrypted
keystore file to the wallet fob; do not use `wallet export`, which writes
the private key in plaintext. Send the founder only the account address that
`wallet info` prints. The genesis credits it with your 20,000 DGT. Moving or
staking DGT costs a fee in DRT, so the account also needs a little DRT (the
DRT bootstrap, D08-Q03).

## Afterwards: the founder, online

1. Collect the five public sticks into the custody working folder, not this
   repository.
2. **Genesis signer policy.** From the five kits' genesis keys (public
   inputs only; run it on the ceremony machine or online):

   ```text
   cd FOLDER && dytallix-root-sign policy -chain-id dytallix-mainnet-1 \
     -out genesis-signer-policy.json kit-1-genesis.json kit-2-genesis.json \
     kit-3-genesis.json kit-4-genesis.json kit-5-genesis.json
   ```

3. Fill the three intakes in order: the [emergency](emergency/INTAKE.md),
   [upgrade](upgrade/INTAKE.md) and [genesis](genesis/INTAKE.md) packets.
   Slot N uses kit N's public records, `kit-N` as its control group and the
   holder's controller ID. The packet model for five holders is still to be
   set (see the intakes). Each proof is the public statement behind its
   `proof_of_possession` evidence; the signer and backup records say that
   the key is on kit N's encrypted fob, with no backup, without saying where
   it is. The drill records come from a staging freeze and resume signed with
   the kits ([control signing](../../node/docs/mainnet/control-signing.md)).
4. Run the three checkers. Their fragments go into the genesis records.

## A stolen or exposed kit

Treat its four keys as known. One kit is below every threshold; follow the
[key compromise runbook](../../node/docs/operations/key-compromise.md) and
replace the kit before a second is at risk.

## Node keys

Once the hosts are rented and the pin plan has their addresses
([host setup v1](../../node/docs/architecture/host-setup-v1.md)), the founder
runs a second offline session on the same live USB. It makes every host's
node keys and seals them for its bundle (P01, 6 October 2026). The tools
stick also holds the release's `dytallix-peer-seed`,
`dytallix-validator-key` and `dytallix-channel-key`, and
`node/tools/mainnet-preparation/host_keys.py`; the public stick holds the
pin plan. The session needs `python3` on the live USB.

1. Boot offline as in step 1. Check the tools against `SHA256SUMS` as in
   step 2, copy `dytallix-root-sign` and the three key tools to `/tmp/bin`
   and `host_keys.py` to `/tmp`, and mount the public stick at
   `/mnt/public`.
2. Run:

   ```text
   python3 /tmp/host_keys.py --plan /mnt/public/PIN_PLAN.json --bin /tmp/bin \
     --staging /tmp/staging --out /mnt/public/node-keys \
     --backup-upload /tmp/upload.json
   ```

   `/tmp/upload.json` is the off-host store's write-only upload key
   ([disaster recovery v1](../../node/docs/architecture/disaster-recovery-v1.md)):
   type it into the RAM disk with a text editor (`chmod 600`; schema
   `dytallix.backup-upload.v1`: `endpoint`, `bucket`, `region`, `prefix`,
   `access_key_id`, `secret_access_key`). For the sentry, `host_keys.py`
   also makes the chain's backup code and prints its line,
   `dytallix-backup-CHAIN` and seventeen groups: write it on paper twice and
   type it back. Both are sealed with the sentry's keys.

3. For each host it prints a seal code line: `dytallix-seal-LABEL` and
   seventeen groups. Write it on paper twice, then type it back from the
   paper and press Enter; `seal-check` confirms it opens that host's keys.
4. The seal codes and the backup code stay on paper (the root key holders
   approval changes only the kits). Keep the two copies of each in two
   separate places of your own, never with a holder's kit: a code and the
   sealed records together open that host's keys. The staging homes are in
   RAM and go when you shut down. The sealed records on the public stick are
   the node keys' only backup: with one validator, losing its key halts the
   chain.

`/mnt/public/node-keys` then holds only public files: each host's key
summary and sealed keys, the endpoint's channel pin and `PIN_PLAN.json` with
the public keys filled in, for the host files and the genesis.

## Rehearse first

Each holder runs their session once with a throwaway fob and the session ID
`rehearsal`, then wipes it. CI rehearses the same commands with throwaway
kits (`TestKitCeremony` in `dytallix-root-sign`).
