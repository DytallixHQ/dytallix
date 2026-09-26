# Native development supervisor

This package implements the native owner for a disposable Linux service. It does not authorize a production launch. The only accepted mode is `disposable-loopback-native`. Production chain names are refused.

The service performs these steps:

1. Check explicit configuration pins and the private state paths.
2. Acquire the existing home and validator-signing-identity locks. Preserve both lock files.
3. Verify root authorization and committed release history with the independently pinned root helper and read-only storage. Require its explicit private scratch directory before running that helper.
4. Select the candidate digest from verified authority. Verify the complete V2 catalog and configured helper role bindings.
5. Observe the supervisor's actual executable and mapped files.
6. Start and retain the application child. Check its information response against the preflight state. Observe its executable and mapped files.
7. Transfer application pipes to the bridge on descriptors 3 and 4. Observe the bridge.
8. Start and observe the engine. Verify its private RPC peer identity, chain and application state. Start the optional adapter only when the catalog declares it. Verify its owned listener and chain response.
9. Check child health, retained locks and process mappings at the explicit interval. Stop owned children on failure or cancellation.

Child descriptors 5 and 6 retain the lifecycle locks. The bridge inherits descriptors 3 and 4 in addition to these locks. The owner clears child environment variables, then applies the explicit allowlist. No process is adopted from a PID.

Run the binary with `--development-service-config` and one absolute configuration path. `config::NativeServiceConfig` defines the strict input schema. All hash pins, paths, resource limits and listener ports are required inputs. This package contains no production configuration defaults.

The configuration pins the application configuration, application genesis, root configuration, root request and policy, emergency verifier configuration, V2 candidate configuration and five engine inputs. The V2 catalog separately binds executable and library bytes. An input pin proves byte identity; it does not approve a release.

## Qualification limits

Current-process and child observations are snapshots. Periodic observation cannot prevent a mapping that appears and disappears between checks. Unknown file mappings are refused; no general data-file mapping exception exists.

The application still owns the short-lived root and control helper processes. End-to-end helper observation and custody qualification remain separate requirements. An inherited parent-death signal on direct children does not prove cleanup of every possible descendant.

The locks exclude cooperating owners that use the same protected lock directory. They do not prevent an unrelated process from bypassing the supervisor. Protected deployment configuration and service containment remain required.

The readiness probes establish process identity and protocol responses. They do not independently verify consensus proofs. Consensus commitment, restart, handover, final artifact inspection and production acceptance require separate evidence from the integrated candidate.

The `qualification-fixtures` feature adds synthetic process fixtures and Linux tests. It must remain disabled in release artifacts. These fixtures do not establish integrated service qualification.
