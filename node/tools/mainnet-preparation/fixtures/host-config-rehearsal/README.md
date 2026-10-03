# Host configuration rehearsal

Synthetic inputs and outputs of the host configuration generator for the
production-profile genesis rehearsal on the staging chain `dytallix-staging-1`
(`../genesis-production-rehearsal/genesis.json`). Not hosts, addresses, keys
or a pin plan for any network.

- `PIN_PLAN.json`: four operators, four validators with the rehearsal's
  genesis validator keys, five sentries and one endpoint, on documentation
  addresses (RFC 5737). Peer keys and non-validator keys are SHAKE-256 bytes,
  so no private key exists for any of them.
- `host-values.json`, `host-values-resolution.json`: `resolve_host_values.py`
  output from `launch/E05_VALUES.json` and `launch/hosts/PROPOSALS.json`
  (30 approved values, 4 measurement placeholders).
- `PIN_PLAN_BINDINGS.json`: `dytallix-host-config` output for the plan.

`go test ./cmd/dytallix-host-config` (in `consensus/cometbft`) regenerates the
plan and the bindings and compares them; `DYTALLIX_UPDATE_HOST_FIXTURE=1`
rewrites them. `test_resolve_host_values.py` checks the values. See
[host configuration](../../../../docs/mainnet/host-configuration.md).
