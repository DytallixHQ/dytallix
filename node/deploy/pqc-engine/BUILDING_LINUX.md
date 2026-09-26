# Linux application build and service qualification

Use an isolated builder with an explicit storage and memory budget. Keep production hosts outside this procedure. Preserve the existing Cargo lockfile and private signing state.

## Artifact boundary

The core service needs three executables for the same Linux architecture: `dytallix-pqc-engine`, `dytallix-comet-bridge`, and `consensus_stdio`. The selected private IPC profile also needs the HTTP adapter. Root-enabled operation needs the root verification helper. Pin each executable and its providers. A cross-built engine does not establish Rust application compatibility. A successful systemd boot does not establish operation of this service.

The earlier local Rust 1.88.0 build and systemd result used arm64. The recorded Hetzner hosts use x86_64. An arm64 result cannot qualify those hosts. Build and test x86_64 artifacts. Treat execution under emulation as local qualification; production host qualification remains separate.

## Dependency preparation

1. Verify the Rust version against `rust-toolchain.toml`.
2. Pin the build image by its immutable digest. Mount source and shared Cargo registries read-only.
3. Create an owned Cargo overlay. Keep the shared registry unchanged.
4. Run locked, offline dependency resolution for the selected target and exact feature set.
5. If an archive is missing, fetch only the exact Cargo.lock version from the public crate registry. Check its SHA256 against the lockfile checksum before adding it to the owned overlay. Do not relax certificate verification.
6. Record each source URL, version, byte count, archive hash, and checksum comparison. Apply explicit individual and total download limits.
7. Keep the build container network disabled. Run the build with both `--locked` and `--offline`.

The earlier component build selected `--no-default-features --features pqc-fips204,metrics,oracle,contracts` for the `dytallix-fast-node` package and its `consensus_stdio` binary. This explicitly matches the four current default features. That recorded build predates the repaired module gates. Use [the selected Rust application procedure](BUILDING_PQC_APPLICATION.md) for the new `pqc-consensus` profile. Preserve the earlier failure as historical evidence. Neither feature selection establishes production approval.

Do not reuse a native RocksDB archive just because its filename matches. The old cached image has RocksDB 8.10.0, while the current lockfile requires 8.1.1. Those archives are not interchangeable.

## Resource control

Select the build profile explicitly. The earlier bounded component build disabled incremental compilation and debug information. A release qualification must use the declared release profile and record all overrides. Set explicit job, memory, swap, temporary filesystem, log, and deadline limits. Record all settings.

A container temporary filesystem does not guarantee zero host disk growth. The host and VM can consume disk while compilation uses memory. Monitor host free space throughout the run. Include enough shutdown margin above the required reserve. A shared host can consume that margin independently; monitoring cannot guarantee an absolute reserve against unrelated writers.

Stop the owned build before the reserve is exhausted. Record the last sample, shutdown result, remaining free space, and whether a compiler or resource limit caused termination. Remove only owned runtime files. Do not delete shared caches, images, evidence, unrelated containers, or system swap files to force the build to finish.

The first bounded attempt used a 1.2 GiB target filesystem, 4 GiB container memory with no container swap, and two CPUs. It stopped after about 222 seconds because host free space fell toward the 2 GiB reserve. Free space briefly fell below the reserve during shutdown. No application artifact resulted. Increasing the target filesystem alone does not resolve that host limit.

Use additional approved host capacity or a clean builder before retrying. The final build footprint is still unmeasured. Do not assume that the first temporary-filesystem ceiling is sufficient.

When approved capacity is available, use a separate owned disk directory for the target instead of a small temporary filesystem. Keep the source snapshot and shared registries read-only. The current retry uses one CPU, 3 GiB memory, no container swap, and a 2.25 GiB free-space stop threshold above the 2 GiB reserve. Sample free space every 0.5 seconds. Record the peak target size before cleanup. Remove only the owned directory after exporting and verifying the executable. Parent-authorized Docker cache cleanup is separate from this build procedure.

## Service proof

After a successful build:

1. Export the application executable and record its hash, architecture, dynamic dependencies, source pins, lockfile, compiler identity, and exact build options.
2. Verify that source files stayed unchanged during compilation.
3. Create fresh disposable validator and account keys. Keep them out of retained evidence.
4. Supply the existing home, genesis, configuration, and pinned executable manifest required by `supervise.py`.
5. Start the complete stack under the maintained systemd unit as the restricted service identity.
6. Verify the chain identity, validator identities, committed blocks, submitted transaction receipts, and graceful restart through the actual engine RPC endpoint. Test crash recovery separately and record its result.
7. Verify that restart retains database, WAL, and signing-state continuity. Never rewind signing state after later signing.
8. Stop all owned services and containers. Remove private fixture data. Retain public evidence and exact artifact hashes.

A stopped-application inspection must use the service identity and the same `0077` file-creation mask. A database inspection can create files. Loose permissions must trigger refusal on the next service start. Compare numeric JSON heights as integers when one interface uses strings and another uses JSON numbers.

Do not use privileged mode for systemd qualification. A privileged probe changed the shared Docker VM binary-format handlers and interrupted an x86_64 build. A private cgroup namespace did not prevent that effect.

Prepare a separate test image. Mask binary-format, module-loading, sysctl and other unused kernel-management units before starting systemd. Preserve the exact mask inventory. Keep network access disabled and the root filesystem read-only. Retain Docker default device, process-filesystem and kernel-path restrictions. Do not bind the Docker socket, host cgroup tree or host devices.

The qualified image probe added only `SYS_ADMIN` to Docker default capabilities. It remounted only the existing private `/sys/fs/cgroup` filesystem as writable before starting systemd. Record mount information before and after that operation. This is a local test setup with a shared VM kernel. It is not host isolation or production qualification. The maintained application units must run as the restricted service identity with an empty capability set.

Root-enabled operation executes a hash-verified helper snapshot in a private temporary directory. That filesystem must permit execution. The application creates a private directory and a read-only executable snapshot. `PrivateTmp` isolates temporary paths; it does not override a `noexec` mount. Check copied-helper execution under the exact service restrictions before validator startup. Record temporary mount flags and size bounds. An executable temporary test environment does not qualify a `noexec` production environment. A dedicated executable scratch directory and its production policy remain separate deployment choices.

Check ordinary x86_64 execution before and after the probe. Do not probe a changed service image while a cross-architecture build runs. Preserve failures and remove only owned probe containers. Do not change Docker VM settings or binary-format handlers to force a test to pass.

Retain failures with their exact scope. Production remains NO GO until artifact, custody, operator, recovery, and G35 requirements pass.

## Historical arm64 component result

The local arm64 retry built the application and passed 36 checks under actual systemd services. Four validators committed the same transaction, retained its receipt after graceful restart, and resumed blocks with the same peer identities. The native CLI reached the Linux RPC through a loopback byte-forwarding test adapter. The test used the retained TCP loopback engine profile. It did not qualify the newer Unix-socket/Hyper profile, x86_64 hosts, off-host operation, crash recovery, or final launch simulations.

The exported application links to `libssl.so.3` and `libcrypto.so.3`. Exact provider hashes and exported RSA, ECDSA, ECDH, and TLS symbols remain in the retry evidence. Successful service operation does not clear G35.
