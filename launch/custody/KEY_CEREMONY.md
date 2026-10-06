# Key ceremony

Engineering task E05. Under the solo launch profile the founder holds every
root key in five key kits: kit N holds key N of each role (genesis, upgrade,
freeze and resume), and any three kits can act
([solo launch](../approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json),
[trust model](../TRUST_MODEL.md)). The ceremony makes the 20 keys once, on an
offline machine, and takes away only public records and proofs.

The founder chose how (P01, 5 October 2026,
[key ceremony approval](../approvals/P01_E05_KEY_CEREMONY_2026-10-05.json)):

- **Storage.** Each kit is one encrypted USB stick plus a paper copy.
- **Machine.** A laptop booted from a Debian live USB with networking off;
  keys never touch its disk or a networked system.
- **Sites.** The kits are kept in three or more places, so no single fire or
  burglary reaches three kits.

## How a kit works

`dytallix-root-sign kit` makes a random 32-byte kit secret and derives the
kit's four keys from it, one per role (SLH-DSA-SHAKE-256s, FIPS 205 key
generation from seeds derived with SHAKE256). The paper copy is that secret,
written as one line:

```text
dytallix-kit-3 1f0c 9a4e ... (16 groups) 7d21
```

The last group is a check bound to the kit number, so a mistyped digit or the
wrong kit number is caught. The paper line alone recreates all four private
keys, so it is as sensitive as the stick. Losing a stick or a paper copy, but
not both, loses nothing. Losing or exposing two whole kits is survivable;
three is not. The root keys cannot be replaced on chain
([key compromise](../../node/docs/operations/key-compromise.md)).

## What you need

- An x86_64 laptop that can boot from USB. Its own disk is never used.
- A **live USB**: a Debian 12 live image written to a stick, checked against
  Debian's published SHA512SUMS before you write it. It must have
  `cryptsetup`, `mkfs.ext4` and a text editor; check during the rehearsal,
  since nothing can be installed offline.
- A **tools USB**: the release's static Linux `dytallix-root-sign` and its
  `SHA256SUMS` ([release](../../release/README.md)). Before the first release,
  build it with `release/reproduce.sh` and compare with CI's checksums. Check
  the SHA-256 on your online machine and again on the ceremony machine.
- Five **kit sticks**, labeled kit-1 to kit-5.
- One **public stick**, formatted on your online machine (FAT32 or exFAT).
  Only public records and proofs go on it.
- Paper, a pen and five envelopes labeled kit-1 to kit-5.
- Your **controller ID** for the intakes (for example `founder`) and a
  **session ID** for the ceremony (for example `ceremony-2026-10-20`).

## The ceremony

1. **Offline.** Unplug the network cable and turn off Wi-Fi (in the firmware
   settings if you can). Boot the live USB. Never connect to a network until
   you shut down.
2. **Tools.** Mount the tools stick, check the binary against `SHA256SUMS`
   (`sha256sum -c`), and copy it to the RAM disk:
   `install -m 0755 dytallix-root-sign /tmp/`.
3. **Public stick.** Mount it at `/mnt/public` and make
   `/mnt/public/proofs`.
4. **For each kit N, 1 to 5:**
   1. Plug in kit stick N and find it with `lsblk`. Encrypt it, using a
      passphrase for this kit (the paper copy can restore the kit if you
      forget it):

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

      It writes the four private keys and the paper line to the kit stick,
      the four public key records to the public stick, and prints the paper
      line.
   3. **Write the paper line by hand** on kit N's paper.
   4. **Check the paper.** Type what you wrote into a file on the RAM disk and
      check it against the public keys; fix the paper until it matches:

      ```text
      vi /tmp/paper.txt    # or nano
      /tmp/dytallix-root-sign kit-check -paper /tmp/paper.txt -public /mnt/public
      shred -u /tmp/paper.txt
      ```

   5. **Prove possession** of each key. The epoch is 1 for upgrade, freeze
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

   6. Close the stick (`sudo umount /mnt/kit && sudo cryptsetup close kit`),
      and put it with its paper in envelope N.
5. **Genesis signer policy.** From the five kits' genesis keys:

   ```text
   cd /mnt/public && /tmp/dytallix-root-sign policy -chain-id dytallix-mainnet-1 \
     -out genesis-signer-policy.json kit-1-genesis.json kit-2-genesis.json \
     kit-3-genesis.json kit-4-genesis.json kit-5-genesis.json
   ```

6. Unmount the public stick and shut down. The live system's memory is gone
   with the power.
7. **Distribute the kits** over three or more places. Never keep three kits
   in one place, and keep each paper copy sealed with its stick.

## Afterwards, online

1. Copy the public stick to your custody working folder, not this
   repository.
2. Fill the three intakes as `solo_kits`, in order: the
   [emergency](emergency/INTAKE.md), [upgrade](upgrade/INTAKE.md) and
   [genesis](genesis/INTAKE.md) packets. Slot N uses kit N's keys from the
   public records and `kit-N` as its control group. Each proof is the public
   statement behind its `proof_of_possession` evidence; the signer and backup
   records say that the key is on kit N's encrypted stick and its paper copy,
   without saying where they are. Drill records follow once controls can be
   signed in production.
3. Run the three checkers. Their fragments go into the genesis records.

## Recovering a kit

- **Lost or broken stick.** Encrypt a new stick, mount it, and on the offline
  machine with the public records:
  `dytallix-root-sign kit-restore -paper PAPER -public /mnt/public -private-out /mnt/kit`.
  It writes the four private keys only if the paper line derives the
  published keys.
- **Lost paper.** On the offline machine, copy the line from the stick's
  `kit-N-paper.txt` onto new paper and check it with `kit-check`.
- **Stolen or exposed kit.** Treat its four keys as known. One kit is below
  every threshold; follow the
  [key compromise runbook](../../node/docs/operations/key-compromise.md)
  before a second is at risk.

## Rehearse first

Run the whole ceremony once with throwaway sticks and the session ID
`rehearsal`, then wipe them. CI rehearses the same commands with throwaway
kits (`TestKitCeremony` in `dytallix-root-sign`).
