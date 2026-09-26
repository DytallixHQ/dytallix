# Transaction record contract, version 1

This contract covers durable records for the selected development node. It preserves
original parsed signed inputs and normalized execution inputs. It does not define a
mainnet signing protocol or approve transaction authorization.

## Format

The existing `tx:<hash>` key now stores the byte prefix `DYT-TX-RECORD` followed by a
zero byte and a JSON object. The object contains version 1, the normalized transaction,
and an optional parsed signed envelope. The envelope contains the original protocol
transaction, public key, signature, algorithm name, and signed-envelope version.

The original HTTP bytes, whitespace, and unknown input fields are not retained. The
node signs and verifies a canonical encoding of the parsed protocol transaction.
The record retains that parsed input, including original denomination aliases, amounts,
message order, memo, chain ID, nonce, fee, algorithm, and version. Normalization does not
replace the original input in the record. Monetary amounts remain decimal strings.

Tagged message variants use JSON. The former binary serializer can encode those variants
but cannot decode them through their tagged Serde representation. A normal round-trip
control demonstrates this limitation. The new codec handles all five stored message
variants and full-width u128 values. It rejects unknown record versions, malformed
records, a mismatched key hash, and inconsistent shared envelope fields.

The codec checks the original protocol transaction hash against the normalized hash.
It also checks nonce, fee, chain ID, memo, first sender, public key, and signature fields.
It requires the current signed-envelope version and a nonempty algorithm identifier.
It does not verify cryptographic signatures, algorithm activation, key ownership, or every
normalization rule. Those checks belong to the selected validation and replay layer.

## Write and read order

1. Public submission verifies the parsed signed transaction through the existing validator.
2. Submission builds the existing normalized execution transaction and a candidate queue.
3. Storage checks any existing record for the same hash. Its normalized body must match.
   A supplied original envelope must also match an existing envelope.
4. One synchronous batch writes the versioned record and Pending receipt.
5. Submission publishes the candidate queue after the write succeeds.
6. Transaction or block settlement adds the final receipt and preserves the record in the
   same synchronous batch as its other committed effects.

A write without a new envelope preserves an existing envelope. Reusing a hash with a changed
body or supplied envelope fails before publication. An unsigned record cannot gain a different
original envelope through ordinary resubmission. Such repair requires an explicit migration.
Retry a pending submission with the same parsed envelope. A new signature can be a conflict
even when the protocol body is unchanged. The isolated `put_tx` helper uses the same record planner and synchronous storage options.
Its body cannot overwrite a conflicting stored body. Invalid receipt bytes return an error.

Selected block settlement requires an original envelope when a transaction contains a public
key or signature. Recovery requires a readable record for each committed transaction. It
compares the normalized body with the body committed in the block and applies the same envelope
presence rule. Internal unsigned transactions remain possible in the explicit development
profile. They do not qualify as authenticated mainnet inputs.

The record is an index beside the existing committed block body. Pending records remain outside
the selected-state digest, because admission can occur between blocks. Recovery compares each
committed index body against its block. This batch does not create a new consensus commitment
to every envelope field or perform full cryptographic replay at startup.

`Storage::get_transaction_record` returns a record, absence, or an error. It does not convert
invalid records into absence. `GET /api/transactions/:hash/record` returns the stored object,
HTTP 404 for absence, or HTTP 500 for invalid storage. The existing receipt query remains intact.
A record may describe a Pending transaction; its presence does not prove block inclusion.
Use the receipt and verified block journal to determine settlement status.

## Migration and limits

The decoder does not guess old binary layouts or silently reinterpret unknown versions.
It reports that migration is required and leaves the old bytes unchanged. Older block
histories may lack readable transaction indexes or original signed envelopes. The new recovery
check rejects those histories. A reviewed migration must establish data provenance before
writing replacement records. A normalized block body cannot reconstruct lost original aliases,
algorithm metadata, or signatures over a different representation. No migration ran in this batch.

The public fixture uses temporary funded genesis and generated fixture keys. Its test submits
a signed whole-token transfer, checks the stored original and normalized amounts, commits a
block, retrieves the record, reopens storage, verifies recovery, and verifies the restored
original signature. This establishes that local record path. It does not establish signer
ownership, post-quantum interoperability, all-algorithm policy enforcement, or distributed replay.

Pending queue reconstruction remains open. So do normalized-input revalidation, consensus
validation, complete envelope commitments, migration, query and history performance, vesting,
stake transitions, adaptive monetary settlement, independent security review, and launch
qualification. The approved mainnet emission target remains the corrected adaptive model.
