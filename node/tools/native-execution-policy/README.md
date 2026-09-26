# Native execution policy renderer

This tool renders staging files. It does not verify host files, establish release authority, load AppArmor, start a service, or accept G35.

Run Python 3.10 or later:

```text
python3 render.py --catalog catalog.json --mapping mapping.json --request request.json --output new-output-directory
python3 -m unittest -v test_render.py
```

The output directory must not exist. Validation failure creates no output directory. An I/O failure during writing can leave an incomplete directory; reject it unless its complete file manifest verifies.

## Inputs

`catalog.json` uses the existing release-runtime `ManifestV2` structure (schema 2). Its exact input bytes must match `request.catalog_sha512`. Supported service profiles are `development-linux-native-service-v2` and `development-linux-native-service-http-v2`. The target must be Linux x86_64 GNU. Members must be executable files or shared libraries. The renderer rejects script and interpreted members. Runtime-profile `member_ids` contain shared-library IDs only. Empty lists are valid for static executables. Each role selects its executable through `role.member_id`; that executable is not listed as a runtime provider. Combined role and provider references must cover every catalog member, and every runtime profile must be used.

`mapping.json` uses the existing `LocalMapping` structure (schema 1):

```json
{"schema":1,"members":[{"id":"catalog-member-id","path":"/exact/runtime/file"}]}
```

Every catalog member needs one path. Optional aliases must identify an existing member and a distinct exact path. No path grants a directory. The renderer cannot determine whether an unavailable remote path is a regular file. The live verifier must prove that fact before use. Catalog bytes, hashes, and structural validation alone do not establish approval. The caller must also use the maintained release-runtime validator with trusted expected release inputs.

The request has these exact fields:

```json
{
  "schema": 1,
  "policy_version": "native-library-staging-v1",
  "catalog_sha512": "128 lowercase hexadecimal characters",
  "service_uids": [41001, 41002, 41003, 41004],
  "writable_roots": ["/var/lib/example/state", "/run/example/runtime"],
  "readonly_files": ["/opt/example/service.json"],
  "code_aliases": [],
  "devices": ["/dev/null", "/dev/urandom"],
  "network": {
    "mode": "shared-private",
    "namespace_path": "/run/netns/example-owned-anchor",
    "sockets": [
      {"family": "unix", "type": "stream"},
      {"family": "inet", "type": "stream"}
    ]
  }
}
```

These are illustrative paths and UIDs, not approved records. `network.mode=private` requires `namespace_path=null`. It creates a separate private network per unit. `shared-private` uses the named external anchor namespace. The live verifier must establish its ownership and isolation. Socket families are unix, inet, or inet6. Socket types are stream or dgram. This profile controls families and types; it does not claim per-address or per-port filtering. An explicit empty sockets list grants no AppArmor network access and denies socket/socketpair syscalls. It omits RestrictAddressFamilies instead of emitting an empty value that resets restrictions.

The displayed fields are required. Optional `readonly_directories` accepts exact absolute directory paths. Each receives only a trailing-slash read rule, without descendants or execution. A directory cannot equal a code/file record or lie inside a writable root. An ancestor of a writable root is allowed because this rule grants only directory reading; the writable child remains explicit. Empty `readonly_files`, `code_aliases`, and `devices` lists are permitted. Arrays have bounded sizes. Paths must be absolute, normalized, and free of spaces, control characters, glob syntax, specifiers, and mount-field separators. Writable roots cannot contain code, overlap each other, or designate a broad system root. Aliases cannot collide. Unknown fields and duplicate JSON keys are rejected.

## Stable output contract

- `unit-properties.json`: schema 1, profile name, policy ID, permitted numeric UIDs, and systemd property values.
- `apparmor.profile`: one owned profile named `dyt-native-staging-` plus the first 24 hexadecimal characters of the policy ID.
- `validation.json`: member/path/digest bindings and explicit false authority, host verification, enforcement qualification, and G35 fields.
- `live-verification-requirements.json`: mandatory host and workload checks. This file is not an attestation.
- `normalized-request.json`: deterministic request record.
- `FILE_HASHES.json`: exact SHA256 and size for the other five output files.

The policy ID binds the exact catalog digest and normalized local inputs. List ordering does not change rendering. Changing catalog bytes changes the identity. Verify the output hash manifest before integration.

Systemd property arrays represent one space-joined property value. Booleans serialize as `yes` or `no`. An empty capability list means `CapabilityBoundingSet=`. `SystemCallFilter` is one complete deny expression starting with `~`; do not turn its entries into separate allow expressions. Do not quote JSON directly into shell text. Use structured process arguments.

The wrapper selects one numeric `User` value from `service_uids` per unit. It may add resource limits, lifecycle settings, and already-approved descriptors/environment. It must not replace or weaken generated controls. Reject any conflicting wrapper property. Keep the Python harness outside the workload unit and catalog.

The paired policy permits executable mappings only for exact catalog paths. Workload executables inherit the profile. Data permissions do not grant executable mappings. Default denial excludes profile changes and unlisted execution. Proc-memory writes and modifying ptrace operations have explicit denials. Read-only owned-process observation remains permitted, including the exact current-profile attribute and tcp/tcp6 listener metadata. The tcp/tcp6 entries are root-owned even for non-root processes on the selected host, so their exact read rules have no owner qualifier. This grants read-only numeric-PID network table access. Readiness code must still bind the selected PID to its owned child and the expected namespace; the file rule itself does not enforce that relationship. No proc-memory write permission is granted. The AppArmor text has no broad library abstraction or unconfined/fallback transition.

Descriptor import through recvmsg, recvmmsg, and pidfd_getfd is denied. io_uring is denied so that asynchronous message receive cannot bypass that syscall selection. pidfd_open remains available for owned-process observation. These restrictions can reject legitimate socket behavior. Qualify exact Go/Rust paths before use; do not silently relax them. The launcher must also close undeclared inherited descriptors.

## Required live work

The separate `production_roles.py` renderer produces four AppArmor profiles per
unit: supervisor, application owner, workload, and helper. It uses the same
validated catalog, mapping, request, and base policy controls. Its schema 2
admission record binds each catalog role to one exact profile. The service
reads the kernel's loaded profile list and rejects a missing profile or a
profile outside enforce mode before it starts a child. This source candidate
does not load profiles. The directed `Px` transitions and signal rules require
Linux parser and kernel tests with `NoNewPrivileges` enabled.

Verify code files, aliases, ownership, modes, ACLs, protected parent paths, immutable backing, effective mounts, kernel support, loaded profile identity, and enforce mode before workload execution. A `NoExecPaths` string or AppArmor profile name is not proof of enforcement. Systemd can ignore AppArmorProfile when AppArmor is disabled. Require independent host checks. Keep data mounts non-executable and all code files read-only.

The root helper must execute from its immutable catalog path with the qualified handshake. The old writable scratch executable conflicts with this policy. Do not grant executable permission to scratch as a workaround.

Run a Linux AppArmor parser check, inert permission probes, and exact workload startup/commitment/receipt/shutdown/restart qualification. These checks are not run by this renderer. Keep provider-source assurance and mapping observations separate. No rendering result changes `continuous_enforcement` or launch gate status.

The initial synthetic control catalogs predate this schema correction. They remain separate finite control fixtures. This renderer no longer accepts executable IDs in runtime-profile provider lists. Do not modify a signed catalog to fit an earlier renderer.

Writable mount noexec rule: emit `/` and every declared writable root in `NoExecPaths`. Keep those same roots in `ReadWritePaths`. Native control run 04 showed that the explicit writable data mount remained executable with only `/` in `NoExecPaths`. A noexec parent does not establish the child's effective mount flags. Before readiness, the owner must check every declared writable root and backing mount for writable and noexec flags. Exact code paths and AppArmor execution grants remain unchanged. Local rendering does not prove native mount enforcement.

Set `SystemCallErrorNumber=EPERM` with the exact denied system-call set. A denied call must return an explicit refusal so the bounded probe can record the result. Do not weaken the denial list or count a killed probe as a recorded denial pass. Native control run 05 terminated deny-memfd with SIGSYS under the default action; its cleanup passed. The explicit errno still requires native verification.
