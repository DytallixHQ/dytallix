# Registry test contract

This contract exercises the existing node host ABI. It parses registry requests,
assigns sequential asset IDs, writes each asset through host storage, and reads
assets on later calls. It returns `null` for an unknown ID.

Run `python3 scripts/build_registry_fixture.py` from the repository root before
node integration tests. Install the `wasm32-unknown-unknown` target for the pinned
Rust toolchain first. The script respects Cargo's configured target directory.
CI performs this build before the workspace tests. The generated WASM is ignored.

This fixture tests calls within one runtime instance. It does not establish
contract isolation, authenticated caller identity, persistent node storage,
crash recovery, or mainnet contract support. The current node uses a placeholder
caller; the test records this limitation explicitly.

The fixture build is a prerequisite for `cargo test --workspace --all-targets`.
A clean source archive does not contain the generated WASM. The release binary
does not depend on this test contract.
