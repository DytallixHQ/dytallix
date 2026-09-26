# Development release runtime

This crate provides strict V2 member-file validation and bounded observation of caller-owned Linux children. It does not approve a production release or satisfy G35.

`component_candidate` exposes `ExpectedRelease`, explicit bounds and file policy, strict canonical manifests/mappings, and opaque validated/file results. Expected release authority and the bootstrap verifier pin must come from independent trusted inputs. Local paths and a manifest's own digest do not establish authority.

Supported profiles:

| Profile | Supervisor | Required additional roles |
|---|---|---|
| `development-linux-python-service-v2` | Script and interpreter | Application, bridge, engine, bootstrap verifier, live verifier |
| `development-linux-python-service-http-v2` | Script and interpreter | Same roles plus HTTP adapter |
| `development-linux-native-service-v2` | Native executable | Application, bridge, engine, bootstrap verifier, live verifier |
| `development-linux-native-service-http-v2` | Native executable | Same roles plus HTTP adapter |

The native profiles reject interpreter roles and interpreted runtime members. Selecting a native profile does not prove that an installed Python service has been replaced. All actual deployed roles require separate launch and behavior qualification. Bootstrap and live verifier roles can reference the same or distinct accepted files. The bootstrap pin remains independent in both cases.

`observation` exposes opaque process identity, private snapshot, and successful observation objects. Successful observation has read-only accessors and serializable report output. It has no public constructor or deserializer. Report data cannot become a trusted observation object.

Observation accepts only caller-owned child handles. It checks Linux process start identity, pidfd state, executable identity, and bounded identity-bound file mappings. Unknown mapped files, including database data maps, remain unsupported. A separate explicit data-mapping policy is required before real database processes qualify. An observation is a snapshot; it does not prevent later or transient loading. Interpreter executable checks do not establish script or import execution.

Private snapshots preserve member content identity while using a separate owned inode. Their scratch filesystem must permit execution. A file mode cannot override a noexec mount. The deployment wrapper must verify scratch policy without widening privileges.

## Tests

Run portable catalog/observer and exact V1 compatibility tests:

```
cargo test -p dytallix-release-runtime --offline --locked --lib
```

The copied V1 source is test-only. The node's maintained V1 implementation is unchanged.

The `qualification-fixtures` feature includes the owned blocking-child binary and five Linux process tests. It is off by default. On a qualified native Linux host, build the fixture and run with its exact absolute path in `DYT_OBSERVER_CHILD`. Use a private executable-capable `TMPDIR`, explicit external time/resource limits, and the approved confinement. Do not enable this fixture feature for production artifacts.

```
cargo build -p dytallix-release-runtime --offline --locked --features qualification-fixtures --bin release-runtime-owned-child
DYT_OBSERVER_CHILD=/absolute/path/to/release-runtime-owned-child cargo test -p dytallix-release-runtime --offline --locked --features qualification-fixtures --lib
```

A cross-platform build or portable test run does not count as native process evidence. Prior isolated harness results do not automatically qualify this maintained crate, its changed libc dependency, or a service integration.
