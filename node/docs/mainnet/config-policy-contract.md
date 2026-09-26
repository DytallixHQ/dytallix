# Configuration loading and signature policy contract

## Boundary and acceptance

`NodeConfig::load_with_secrets` returns a configuration only after `validate` succeeds.
`ConfigLoader::load_node_config` uses the same path. API and JWT credentials are required.
Blank credentials, placeholder values, and the known `stub_` credential family fail
validation. JWT values need at least 32 bytes after trimming surrounding whitespace.
This length check does not establish entropy, key custody, or suitability for a JWT
algorithm.

Present numeric and Boolean settings must parse. Missing optional settings use defaults.
A zero API port, zero peer port, zero rate limit, empty bind address, empty database URL,
zero database pool size, empty key path, or unsupported TLS version fails validation.
Accepted TLS version labels are `1.2` and `1.3`. Log levels must parse as a tracing
level: `trace`, `debug`, `info`, `warn`, or `error` (case-insensitive). The preferred signature algorithm must
belong to the configured allowlist. These checks do not verify actual TLS enforcement,
network binding, database access, key-file integrity, or cryptographic backend support.

Missing database credentials retain the existing SQLite default. A present database
password must be nonblank and must not use a recognized placeholder. PostgreSQL URL
construction encodes credentials and database names as components. Reserved characters
cannot change the host, query, or fragment through those components. This is URL
construction evidence, not a live database connection test.

`is_production_ready` now requires validation, TLS, audit logging, disabled debug mode,
legacy signature rejection, and policy enforcement at both mempool and consensus.
Its name is retained for API compatibility. It checks configuration fields only.
It does not establish that a node uses those fields or is ready for mainnet.

Public fields and Serde deserialization still permit an unvalidated configuration.
The legacy `load_from_env` method still returns an unvalidated template. Consumers of
those APIs must call `validate` before use. `Default` is a template with placeholders;
it is deliberately invalid as a loaded credential configuration.

## Secret providers and diagnostics

The environment provider preserves empty values as present. It still adds its prefix
and applies its configured case rule. It does not replace `/` with `_`: for example,
`api/api_key` with the normal prefix resolves to `DYTALLIX_API/API_KEY`.

The manager retains provider order and fallback to later providers. A later successful
lookup remains valid. If no provider supplies the requested value, any lookup failure,
timeout, or skipped initialization failure prevents a `NotFound` result. The manager
returns a provider error instead. The strict loader converts that error into a
configuration error and does not supply a default.

The compatibility helper `get_secret_or_default` still converts all errors into its
caller's default. The strict node loader does not use it. Other callers require their
own policy. The existing cache may still serve a valid cached value during provider
failure. Provider refresh, secret rotation, and revocation require separate work.

`NodeConfig` Debug output redacts the database URL, API key, and JWT secret. Loader
parsing errors identify field names without values. Manager failure logs omit raw
provider error details. Explicit configuration serialization still contains secrets;
this change does not make serialized configuration suitable for public output.

Vault remains a development stub. The manager still lacks a production Vault backend.
The stub's known credentials cannot pass this loader. This batch does not qualify a
secret provider, enable deployment, or establish production custody.

## Algorithm parsing and permissions

The default allowlist remains Dilithium3 only. Name parsing is now separate from
permission checks. Parsing a recognized name grants no permission to use it.
`validate_algorithm_name` still applies the selected policy.

An explicit replacement list parses every comma-separated item before assignment.
An empty list, empty item, legacy name, or unknown name returns an error. Existing
PQC aliases and case-insensitive names remain supported. Duplicate names collapse
into the existing set type. A preferred algorithm outside the replacement list fails
configuration validation.

An explicit wider allowlist remains a supported configuration API. This does not
change network defaults or prove that every named backend is usable. Node activation
must use one approved network policy consistently across admission and consensus.

## Runtime and genesis limits

Repository caller review found no executable entry point that loads this `NodeConfig`.
The core consensus engine creates `PolicyManager::default` directly. The selected
fast node also has its own configuration and mempool policy path. Runtime integration
and agreement between validator configurations remain open mainnet requirements.

Genesis inspection found inconsistent unit descriptions and amounts. The core genesis
template totals 10^18 smallest units, while three integration assertions use 10^27.
The template comments also mention incompatible decimal scales. Two tests assume a
vesting cliff based on the current wall clock. The genesis amount-string test also
fails because amount fields lack the required serialization helper.

This batch does not rescale genesis balances or change those expected totals. A reviewed
unit contract, canonical genesis encoding, deterministic time input, checked vesting
arithmetic, and agreement with the selected chain implementation remain required.
The selected adaptive emission model and unresolved allocation decision remain intact.
