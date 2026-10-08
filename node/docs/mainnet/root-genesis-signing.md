# Root genesis signing (three of five)

Production activation v1, step A2. A production node opens a chain only from
a root genesis signed by three of the five genesis signers (P01, 30 September
2026, `launch/approvals/P01_E05_ACTIVATION_2026-09-30.json`). This page is
the signers' procedure with the offline signer,
`consensus/root-authorization/cmd/dytallix-root-sign`. It does not appoint
signers or authorize a launch. Root signers sign only after the P02 review,
E06 release acceptance and gate acceptance. The five genesis keys are in
the five key kits, held by five people, one each (P01, 7 October 2026,
`launch/approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json`). Under the solo
launch profile (P01, 3 October 2026) the P02 review is the 30-day public
review and the founder's sign-off (`launch/TRUST_MODEL.md`).

## What is signed

The root bundle binds four files:

- the native genesis;
- the application configuration;
- the engine genesis (by SHA-512);
- the release manifest (by SHA-512).

Every signer signs one genesis envelope over the bundle's SHA-512: the chain
ID, action `genesis`, sequence 1 and heights 0 to 0. The signature is
SLH-DSA-SHAKE-256s, 29,792 bytes, under the root action context
`DYTALLIX/ROOT/v1/genesis`.

## Public records

Both records are public and canonical JSON. Publish them with the network
configuration and give every node the same bytes.

| Record | Contents |
| --- | --- |
| Signer policy | Schema 1, the chain ID, five keys `{key_id, public_key_hex}` sorted by key ID, threshold 3. `key_id` is the lowercase SHA-256 of the public key. |
| Signatures | Schema 1, the chain ID, the bundle SHA-512 and three to five `{key_id, signature_hex}` entries sorted by key ID. |

The node keeps its receipt in consensus state, and the receipt lists the
signing key IDs and the signatures file's SHA-256. Every node must therefore
read the same combined file. Two different valid files (for example three
signatures and four) start two different chains.

The genesis signers are a separate group: their keys must not hold an
emergency, upgrade or handover role. The node refuses to open a chain whose
genesis signer policy shares a key with any of them.

## Procedure

Each signer works on their own device. Private keys never leave it and never
enter this repository, the custody packet or a ticket.

1. **Key.** Each signer generates a key once, in the approved key ceremony:

   ```text
   dytallix-root-sign keygen -private-key-out signer.private -public-key-out signer.json
   ```

   It writes the 128-byte private key (mode 0600) and the public key record,
   and prints the key ID. Back up the private key under the custody
   procedure. Hand over only `signer.json`.

   The genesis keys come from the five key kits instead: in the
   [key ceremony](../../../launch/custody/KEY_CEREMONY.md) each holder makes
   `kit-N-genesis.key` and its record `kit-N-genesis.json` for their kit,
   and the policy below takes the five records. Each holder signs on their
   own offline machine and hands over only the signature.

2. **Policy.** The custody lead assembles the five public records for the
   chain, and every signer checks that their own key ID is listed. The
   [genesis signer intake](../../../launch/custody/genesis/INTAKE.md) checker
   emits the same policy, byte for byte, once the signers' records are
   complete and separate from the emergency and upgrade custodians:

   ```text
   dytallix-root-sign policy -chain-id CHAIN -out policy.json a.json b.json c.json d.json e.json
   ```

3. **Digest.** Each signer recomputes the bundle digest from the published
   genesis files. They compare it, and the four file digests it prints,
   with the published build manifest:

   ```text
   dytallix-root-sign digest -native-genesis native-genesis.json -config application-config.json \
     -engine-genesis genesis.json -release-manifest RELEASE_MANIFEST.json
   ```

4. **Sign.** Each signer signs with their own key. The signer refuses a key
   outside the policy, and a private key file readable by anyone but its
   owner:

   ```text
   dytallix-root-sign sign -policy policy.json -private-key signer.private \
     -bundle-sha512 DIGEST -out signer-signature.json
   ```

5. **Combine.** Anyone can merge three to five signature files and verify
   them. Publish the result:

   ```text
   dytallix-root-sign combine -policy policy.json -out root-genesis-signatures.json s1.json s2.json s3.json
   dytallix-root-sign verify -policy policy.json -signatures root-genesis-signatures.json -bundle-sha512 DIGEST
   ```

## The node

The operator's root configuration (`consensus_stdio --root-config`) names:

- the pinned helper and its observed execution policy;
- the two public records;
- the engine genesis and release manifest files, with their SHA-512 digests.

On every start, including restart, the node:

1. checks both records;
2. hashes the actual files;
3. rebuilds the bundle;
4. verifies every listed signature through the pinned `dytallix-root-verify`
   (helper wire version 2), one run per signature.

Any refused signature stops the start. At InitChain it commits a version 2
receipt:

- scope;
- the policy SHA-256;
- the threshold;
- the signing key IDs;
- the bundle SHA-512;
- the envelope and signatures SHA-256.

A later start compares that receipt byte for byte.

## Not covered here

- Appointing the genesis signers and their custody records.
- The key ceremony and proof of possession.
- Backup, replacement and recovery procedures.
- The chain ID and the published network configuration.

These are custody and launch records (D10-Q03, D14-Q03). Records stay in the
custody system; only public keys, key IDs and digests enter this repository.
