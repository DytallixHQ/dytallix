# Testnet relaunch runbook — `dyt-local-1`

Relaunch the single-validator Dytallix testnet from a **fresh genesis** on the
security-hardened `fast-launch` node. Reuses the existing validator key and
keeps the chain-id `dyt-local-1`.

> Depends on the RPC-hardening fixes (PR #5). Build/deploy from that branch (or
> `main` once #5 is merged).

## ⚠️ Read first: the faucet depends on `/dev/faucet`

The faucet service dispenses tokens by calling the node's **`/dev/faucet`**
admin endpoint (`dytallix-faucet/src/controllers/faucetController-dual.js`),
not by signing a transfer from a funded wallet. PR #5 disables `/dev/faucet`,
`/ops/pause`, `/ops/resume` unless `DYT_ENABLE_DEV_ENDPOINTS=true`.

So for the faucet to keep working after the relaunch you must **either**:

- **(now)** run the node with `DYT_ENABLE_DEV_ENDPOINTS=true` **and** ensure the
  node RPC port (3030) is reachable only from localhost / the faucet service
  (firewall it; the public nginx vhost already does not proxy `/dev/faucet` or
  `/ops/*`), **or**
- **(proper follow-up)** rework the faucet to send signed transfers from a
  funded treasury account so the node never needs the admin mint endpoint. Then
  leave `DYT_ENABLE_DEV_ENDPOINTS` unset. Tracked separately.

This runbook uses the first option.

## Pre-flight

- [ ] PR #5 (`security/rpc-hardening`) reviewed/merged; node binary built from it.
- [ ] Validator key preserved (Vault, or sealed keystore at `~/.dytallix/keystore`).
      **Do not delete the keystore** — that is what "reuse existing key" means.
- [ ] `deploy/genesis.json` edited: set the real treasury address + amounts
      (or remove the `accounts` entry for an empty genesis).
- [ ] Confirm the data dir path (`DYT_DATA_DIR`, default `./data`). The chain
      DB is `${DYT_DATA_DIR}/node.db`.
- [ ] Firewall: node RPC port (3030) restricted to localhost / faucet only;
      public ingress only via nginx.

## Relaunch steps

```sh
# 1. Stop the running node (systemd unit name will vary)
sudo systemctl stop dytallix-node

# 2. Deploy the patched binary (built from PR #5)
#    e.g. cargo build --release -p dytallix-fast-node  -> install the artifact

# 3. Wipe ONLY the chain database — keep the validator keystore untouched
rm -rf "${DYT_DATA_DIR:-./data}/node.db"

# 4. Install the fresh genesis next to the node's working dir
#    (main.rs reads ./genesis.json relative to the process CWD)
cp deploy/genesis.json /path/to/node/workdir/genesis.json

# 5. Start with the chain-id + faucet/dev-endpoint flag set
export DYT_CHAIN_ID=dyt-local-1
export DYT_ENABLE_DEV_ENDPOINTS=true            # required for the faucet (see warning)
export DYT_CORS_ORIGINS="https://dytallix.com"  # PR #5: explicit CORS allow-list
sudo systemctl start dytallix-node
```

## Post-relaunch verification

```sh
# Height resets to genesis and climbs; chain-id unchanged
curl -s https://dytallix.com/status | jq '{chain_id, latest_height, status}'
curl -s https://dytallix.com/api/stats | jq '{height, latest_emission}'

# Dangerous endpoints must NOT be reachable publicly (expect non-2xx / nginx block)
curl -s -o /dev/null -w '%{http_code}\n' -X POST https://dytallix.com/dev/faucet

# Faucet still dispenses (drives /dev/faucet over localhost on the node host)
curl -s -X POST https://dytallix.com/api/faucet/request \
  -H 'content-type: application/json' \
  -d '{"address":"dytallix1...","tokenType":"both"}' | jq .
```

- [ ] `latest_height` resets and increases each block.
- [ ] `circulating_supply` starts at the genesis total (0 + treasury) and grows by emission.
- [ ] Public `POST /dev/faucet` is rejected; faucet requests via `/api/faucet/*` succeed.
- [ ] Update `dytallix-faucet` and the explorer configs if any chain params changed
      (chain-id is unchanged here, so typically no change needed).

## Notes

- There is no fork/activation mechanism; a fresh DB + genesis is the supported
  way to apply consensus-relevant changes on this node.
- The in-code dev prefunds in `main.rs` (e.g. `dyt1senderdev000000`, the test
  accounts) still apply on a fresh DB. Remove those for a clean public testnet
  if undesired — separate change.
