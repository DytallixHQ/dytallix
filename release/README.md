# Dytallix release

Engineering task E06: the reproducible release. Anyone can rebuild the
release from source in the pinned builder and get the same bytes. The
decisions are in the [E06 release approval](../launch/approvals/P01_E06_RELEASE_2026-10-05.json)
(P01, 5 October 2026).

## What is released

[RELEASE_SET.json](RELEASE_SET.json) lists every member, for Linux x86_64:

| Build | Binaries |
| --- | --- |
| Node application (`production` feature) | `consensus_stdio`, `dytallix-genesis-build`, `dytallix-state-check` |
| Native supervisor (`production`) | `dytallix-native-supervisor` |
| HTTP adapter (`production`) | `dytallix-pqc-http-adapter`, `dytallix-channel-key` |
| Engine (`production` tag) | `dytallix-pqc-engine`, `dytallix-comet-bridge`, `dytallix-light-export`, `dytallix-peer-seed`, `dytallix-validator-key`, `dytallix-operator-rpc`, `dytallix-host-config` |
| Root authorization | `dytallix-root-verify`, `dytallix-root-sign` |
| CLI wallet | `dytallix` |

`catalog_roles` maps each production catalog role to its binary; both
verifier roles use the root helper. The CLI wallet is the launch wallet. On
macOS and other systems, install it from the release's source tag:
`cargo install --locked --path sdk/crates/dytallix-cli`. The static Linux
wallet looks up host names with glibc's built-in hosts-file and DNS
resolver; on a system whose name service needs other plugins (mDNS,
systemd-resolved's NSS module), use an IP address or install from source.

There is no node container: nodes run natively under the supervisor. The
release publishes its builder image so anyone can reproduce it.

## The builder

[builder/Dockerfile](builder/Dockerfile) pins every input:

- **Rust.** `rust:1.88.0-slim-bookworm` by image digest, the version in
  `rust-toolchain.toml`.
- **C and C++** for RocksDB, bzip2, zlib and lz4: clang, libclang, g++ and
  binutils from snapshot.debian.org, frozen at 21 July 2025 (the base
  image's own Debian date).
- **Go.** 1.25.14, the latest 1.25 patch, checked against its published
  SHA-256.

The image itself need not be byte-identical. Each build records the
toolchain versions it used.

## Build

[build_release.py](build_release.py) runs inside the builder with the mainnet
tree read-only at `/src`. It builds each member with fixed settings:

- Rust: `cargo build --locked --release --target x86_64-unknown-linux-gnu`
  with glibc and libstdc++ linked in statically (`+crt-static`), no
  incremental build, and the source and registry paths remapped out of the
  binaries.
- Go: `-trimpath -buildvcs=false -ldflags=-buildid=` and `CGO_ENABLED=0`.

Every member is a static executable: it loads no shared library and names
no program interpreter, and the build fails otherwise (P01, 5 October
2026). So a host's operating system updates never change what the node
runs, and the release catalog lists only the release's own binaries.
Library fixes ship as new releases.

It writes, with no time, host name or builder path:

- `bin/`, the binaries;
- `SHA256SUMS` and `SHA512SUMS`;
- `BUILD_RECORD.json`: toolchain versions, lockfile and migration registry
  digests, and each member's size and digests;
- `SBOM.json`: every Rust crate and Go module the lockfiles pin.

## Reproduce

- **CI.** [mainnet-release.yml](../../.github/workflows/mainnet-release.yml)
  builds the set twice, on separate runners, and requires identical
  checksums, build records and SBOMs.
- **Your machine.** With Docker running:

  ```text
  release/reproduce.sh OUT_DIR [CI_BUILD/SHA256SUMS]
  ```

  It builds offline from your local crate and Go module caches, which Cargo
  and Go check against the lockfiles, then compares with another build's
  checksums. On an arm64 Mac the x86_64 builder runs under emulation:
  slower, same bytes. Build caches stay in the Docker volume
  `dytallix-release-work` until you remove it.

## Authority

A release's authority is the chain, not a signature on GitHub, since every
GitHub signature is classical cryptography:

- **First release.** The genesis binds the release manifest's SHA-512, and
  the 3-of-5 genesis keys sign the genesis.
- **Later releases.** The 3-of-5 upgrade keys approve each one on-chain.

Checksums and tags help people find and check files; they authorize nothing.

## Release manifest

The release manifest is the supervisor's catalog: release-runtime's
`ManifestV2` for the production service profile. It names the chain, the
native genesis digest and the compiled migration registry, and lists the
binary for each catalog role, with no runtime libraries.
[release_manifest.py](release_manifest.py) writes it from the release's
`BUILD_RECORD.json` and the genesis builder's `native-genesis.json`, in the
exact encoding the supervisor checks, and prints its SHA-512 and the root
helper pin.

The native genesis does not depend on the manifest, so at the final freeze:

1. Build the genesis and take `native-genesis.json`.
2. Write the manifest:

   ```text
   release/release_manifest.py write --record BUILD_RECORD.json \
     --native-genesis native-genesis.json --out RELEASE_MANIFEST.json
   ```

3. Set `root.release_sha512` in `GENESIS_INPUTS.json` to the printed
   `release_sha512`, build the genesis again (its `native-genesis.json` is
   unchanged) and have the 3-of-5 genesis keys sign it.

`release_manifest.py check` rebuilds the manifest from the same inputs and
exits 1 unless the bytes match.

## Publication

Releases are published as GitHub Releases on `DytallixHQ/dytallix`, after
mainnet/ moves there with its history. They're tagged once, in their final
home.
