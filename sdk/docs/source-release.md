# Standalone SDK source release

The SDK workspace includes `vendor/dytallix-protocol-types`. This directory is
an exact copy of the approved canonical node protocol crate, its tests, and its
public vectors. The included MIT license comes from the node repository. The SDK
can build without a sibling node checkout.

`vendor/protocol-types-source.json` identifies every vendored file by size and
SHA-256. It records the node repository HEAD for context. That HEAD does not
contain all approved local changes. The manifest states this limit explicitly.
The file hashes identify the complete working-tree snapshot used here.

Do not edit the vendored codec or regenerate its vectors independently. When the
canonical node crate changes, review that change, copy the exact complete crate,
and update the provenance manifest. Update or qualify the SDK against the new
snapshot before release.

## Verify source integrity

From the SDK directory:

```sh
python3 scripts/check_protocol_vendor.py
python3 scripts/test_source_packaging.py
```

The first command works without Git or a node checkout. It rejects changed,
missing, extra, or symlinked vendor files. To compare the source checkout as well:

```sh
python3 scripts/check_protocol_vendor.py --node-root /absolute/path/to/dytallix-node
```

This optional check also rejects changes to the canonical crate file inventory.
It reports drift. It does not copy files or approve a new protocol version.

## Create the source archive

Use a new output path outside the repository:

```sh
python3 scripts/package_sdk_source.py --output /tmp/dytallix-sdk-source.tar.gz
```

The script checks vendor integrity before packaging. It includes the workspace
crates, vendored protocol, examples, documentation, scripts, workflows, lockfile,
licenses, and toolchain file. It excludes build output, Git metadata, Python
caches, and node_modules. It rejects source symlinks and existing output files.
It writes a per-file `SOURCE-MANIFEST.json` inside the archive and reports the
archive SHA-256. Fixed archive metadata makes identical source inputs reproducible.

This is a source workspace archive. It is not a crates.io package or publication.
The SDK and CLI still use local dependencies within the archive. Registry
libraries are locked in Cargo.lock but are not vendored. Builds need those pinned
libraries from a populated Cargo cache or the registry.

## Qualify an isolated extraction

Extract into a new directory with no sibling `dytallix-node` directory. Verify the
archive SHA-256 and its `SOURCE-MANIFEST.json` file inventory before building.
Run these commands from the extracted `dytallix-sdk` directory:

```sh
python3 scripts/check_protocol_vendor.py
python3 scripts/test_source_packaging.py
cargo +1.88.0 check --locked --offline --workspace --all-targets
cargo +1.88.0 test --locked --offline --workspace
cargo +1.88.0 test --locked --offline -p dytallix-cli --no-default-features --features ordinary-http-only --bin dytallix-ordinary-local
cargo +1.88.0 test --locked --offline -p dytallix-sdk --no-default-features --features ordinary-http-only
```

The legacy HTTPS and local HTTP profiles are mutually exclusive. Do not use
`--all-features`. Qualify the two profiles separately.

Use `--offline` only when the pinned dependencies are cached. A cache miss is a
missing prerequisite, not permission to change the lockfile. For low-disk local
qualification, use an explicit shared `CARGO_TARGET_DIR` and disable test/debug
symbols through environment variables. Do not copy an existing target directory
into the source archive.

Record the archive hash, extracted manifest check, Cargo lockfile hash, toolchain,
commands, and results in a new release evidence package. A successful local build
does not establish crates.io publication, remote source availability, browser
signing support, mainnet activation, or engine/validator qualification.

Qualify the strict local profile separately with
`cargo test --locked --offline -p dytallix-cli --no-default-features --features strict-local-mldsa65 --bin dytallix-ordinary-local`
and
`cargo test --locked --offline -p dytallix-sdk --no-default-features --features strict-local-mldsa65`.
A source distribution includes compatibility source and locked dependencies.
Only an explicitly selected and inspected executable has the strict local scope.
A Linux binary bundle must identify the exact target, build, dynamic providers,
qualified operating-system environment, licenses and artifact hashes. One tested
Linux environment does not establish support for other distributions or ABIs.
