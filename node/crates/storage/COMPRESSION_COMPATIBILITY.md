# Storage compression compatibility

`dytallix-storage` explicitly selects the RocksDB LZ4 and Snappy codecs. This applies to the `pqc-consensus` profile and the default profile. Codec support does not depend on the legacy core crate or legacy service features.

The `Storage::open` options remain unchanged. This change preserves the ability to decode existing Snappy SST files. It also makes Snappy available when the existing default options select compression for new tables. An SST file contains persisted database records. The codecs compress data; they do not provide cryptographic authentication or encryption.

The signature algorithm identifier now comes from `dytallix-protocol-types`. Storage does not need the PQC implementation library to serialize this identifier. The enum variants and serialization remain unchanged.

Run the focused compatibility tests:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test --locked --offline -p dytallix-storage \
  --no-default-features --features pqc-consensus \
  --test compression_compatibility -- --nocapture
```

Each test writes 256 compressible records with one codec and flushes them to SST files. It disables the write-ahead log. It checks the SST size and recorded codec name. It then closes the writer and reopens the database twice through `Storage::open`. All values must match, and the original SST bytes must remain unchanged.

This test checks codec compatibility within the locked RocksDB version. It does not qualify production database migrations, cross-version compatibility, Linux deployment, corrupted databases, rollback procedures, or backups. Those checks require the final release and representative database snapshots.
