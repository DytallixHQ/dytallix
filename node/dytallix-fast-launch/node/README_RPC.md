# Dytallix Lean Launch RPC

[Docs hub](../../docs/README.md) | [Build and run](../../docs/build-and-run.md) | [RPC and API docs](../../docs/rpc-and-apis.md)

> Note: Dual-token model active. Upstream tooling (dcli) enforces denoms `DGT` (governance/stake) and `DRT` (rewards). Future RPC endpoints will expose governance/staking modules using DGT.

Status: MV(T) + Persistent Storage + Background Block Producer
Default Port: 3030
Base URL: http://localhost:3030

Environment Flags:
- DYT_DATA_DIR (default: ./data) – Root directory for RocksDB (`node.db`).
- DYT_CHAIN_ID (default: dyt-local-1) – Enforced across restarts (chain ID mismatch aborts).
- DYT_GENESIS_FILE (default: genesisBlock.json) – Used to seed initial balances for any `dyt1...` addresses found in `dgt_allocations` on first boot (height=0 only).
- RUNTIME_MOCKS (default: false) – When true, periodic synthetic events; signature checks relaxed.
- FRONTEND_ORIGIN (optional) – If set, CORS will only allow this origin; otherwise * is allowed.
- MAX_TX_BODY (default: 8192) – Max bytes accepted for POST /submit content length limit.
- DYT_BLOCK_INTERVAL_MS (default 2000) – Interval for background block production.
- DYT_EMPTY_BLOCKS (default true) – If true produce empty blocks when no txs.
- BLOCK_MAX_TX (default 100) – Max txs per block.
- DYT_WS_ENABLED (default true) – Enable /ws websocket.

## Amount / Numeric Types
All large numeric values (balances, amounts, fees) are serialized as strings for JSON safety (u128 friendly).

## Nonce Rules
- Omitted `nonce` on /submit -> server auto uses current sender nonce.
- Provided nonce MUST equal current account nonce or request is rejected 422 with `{ error: "InvalidNonce", expected, got }`.
- On inclusion: sender nonce increments by 1.

## Data Model (RocksDB)
Key prefixes:
- acct:{address} -> bincode(AccountState { balance, nonce })
- blk_hash:{hash} -> bincode(Block)
- blk_num:{u64_be_hex} -> block hash (ascii)
- tx:{hash} -> bincode(Transaction)
- rcpt:{hash} -> bincode(TransactionReceipt)
- meta:chain_id -> chain id string
- meta:height -> u64 (BE bytes)
- meta:best_hash -> latest block hash

## Transaction Receipt
```
{
  hash, status: "success"|"failed"|"pending", block_height, index, fee,
  from, to, amount, nonce, error (optional)
}
```

## State Rules
- Nonce stored per account; must match exactly for inclusion.
- On inclusion of transfer: sender balance -= (amount + fee); sender nonce++ ; recipient balance += amount; fee currently burned (no credit).
- Pending tx stored immediately (status pending until receipt exists).

## Response Envelope (legacy note)
New endpoints return raw JSON objects without a success wrapper (moving toward lean schema). Older docs referenced `{ success, data }`.

## Endpoints
1. POST /submit – submit transfer
2. GET /tx/{hash} – pending or confirmed receipt
3. GET /balance/{address}
4. GET /block/{height|hash|latest}
5. GET /blocks?offset=&limit= – descending from `offset` (or latest)
6. GET /stats – { height, mempool_size, rolling_tps?, chain_id }
7. GET /peers – [] placeholder
8. WS /ws – events `new_transaction`, `new_block`

## Errors (JSON)
`{ "error": "Code", ... }`
- InvalidNonce (422)
- Duplicate (409)
- MempoolFull (429)
- InsufficientBalance (422 inclusion or immediate reject if detectable)
- NotFound (404)

Example error:
```
HTTP 422
{"error":"InvalidNonce","expected":0,"got":2}
```

## Chain ID Persistence
On first startup the node records the `DYT_CHAIN_ID` into RocksDB under `meta:chain_id`. On subsequent boots the provided env value must match stored value or the node aborts to prevent accidental fork / data corruption.

## Example cURL
Submit (auto nonce):
```
curl -X POST http://localhost:3030/submit \
  -H 'Content-Type: application/json' \
  -d '{"from":"dyt1senderdev000000","to":"dyt1receiverdev000","amount":"10","fee":"1"}'
```
Submit (explicit nonce=0):
```
curl -X POST http://localhost:3030/submit \
  -H 'Content-Type: application/json' \
  -d '{"from":"dyt1senderdev000000","to":"dyt1receiverdev000","amount":"10","fee":"1","nonce":0}'
```
Get Tx:
```
curl http://localhost:3030/tx/0xHASH
```
List Blocks:
```
curl 'http://localhost:3030/blocks?limit=5'
```
Get Block:
```
curl http://localhost:3030/block/latest
```
Balance:
```
curl http://localhost:3030/balance/dyt1senderdev000000
```
Stats:
```
curl http://localhost:3030/stats
```

## WebSocket Usage
```
wscat -c ws://localhost:3030/ws
# Example server pushes:
# {"type":"new_transaction","hash":"0x..."}
# {"type":"new_block","height":2,"hash":"0x...","txs":["0x..."]}
```

## Devnet Script
See `scripts/devnet.sh` for an automated E2E proving:
- Block production every DYT_BLOCK_INTERVAL_MS
- Tx lifecycle pending -> success
- Balance / nonce mutation
- Persistence across restart

## Future
- Real signature verification
- Gas accounting / fee distribution
- State root + proofs
- Contract execution layer
- Indexed queries (address tx history)

## AI Risk Oracle (New)
Endpoint: POST /oracle/ai_risk
Payload:
```
{ "tx_hash": "0x...", "score": 0.0-1.0, "signature": "BASE64(ed25519)" }
```
Behavior:
- Stores record under key `oracle:ai:{tx_hash}`.
- If env `AI_ORACLE_PUBKEY` (base64) is set, signature must be a valid ed25519 signature over the ASCII message `"{tx_hash}:{score}"`.
- On `GET /tx/{hash}` the field `ai_risk_score` is included when a record exists (pending or confirmed).
- Survives restarts (persisted in RocksDB).

Local Dev (no signature): omit `AI_ORACLE_PUBKEY` and signature is optional/ignored.

### Example
```
# Post score (dev, unsigned)
curl -X POST localhost:3030/oracle/ai_risk -H 'Content-Type: application/json' \
  -d '{"tx_hash":"0xabc...","score":0.42}'

# Query receipt (after or before inclusion)
curl localhost:3030/tx/0xabc...
# => { ..., "ai_risk_score": 0.42 }
```

### Signed Mode
```
export AI_ORACLE_PUBKEY=BASE64_PUBKEY
curl -X POST localhost:3030/oracle/ai_risk -d '{"tx_hash":"0x...","score":0.9,"signature":"BASE64_SIG"}'
```
Signature generation reference (Python ed25519):
```
import base64, nacl.signing
sk = nacl.signing.SigningKey.generate()
pk = sk.verify_key
msg = f"{tx_hash}:{score}".encode()
sig = sk.sign(msg).signature
print('AI_ORACLE_PUBKEY=', base64.b64encode(bytes(pk)).decode())
print('signature=', base64.b64encode(sig).decode())
```

## External AI Risk Microservice
A minimal service can score transactions and post to the oracle endpoint.
Example (Python FastAPI) in `tools/ai-risk-service` (build & run via included Dockerfile).

Workflow:
1. Client submits tx -> obtains hash.
2. Risk service computes `{score}` using tx fields (heuristic / model).
3. Risk service (or off-chain daemon) signs and POSTs score to `/oracle/ai_risk`.
4. Receipt queries now reflect `ai_risk_score`.

## Keys Added
- `oracle:ai:{tx_hash}` -> JSON { tx_hash, score, signature?, oracle_pubkey? }

## Bridge (New)

Environment:
- BRIDGE_VALIDATORS – JSON array `[{"id":"val1","pubkey":"BASE64_ED25519"}, ...]` loaded once (if no validators stored).

Persistence Keys:
- bridge:halted -> 0|1
- bridge:validators -> JSON array
- bridge:custody:{asset} -> bincode(u128) total locked amount for that asset
- bridge:pending:{id} -> JSON BridgeMessage (before finalize)
- bridge:applied:{id} -> JSON BridgeMessage (after acceptance)

Quorum:
- A bridge message requires >= 2/3 of configured validators (unique) signing the canonical payload:
  `"{id}:{source_chain}:{dest_chain}:{asset}:{amount}:{recipient}"`
- Signatures: ed25519 over ASCII payload, base64 encoded. Order not important; duplicates ignored.

Endpoints:
1. POST /bridge/ingest
```
{
  "id":"0xhash",
  "source_chain":"chainA",
  "dest_chain":"chainB",
  "asset":"dyt",
  "amount":"1000",
  "recipient":"dyt1dest...",
  "signatures":["BASE64_SIG1",...],
  "signers":["val1", ...]
}
```
Responses:
- {"status":"accepted","id":id}
- {"status":"duplicate"} (already processed)
- 500 with error:Internal for quorum failure currently (future: explicit code)

2. POST /bridge/halt
```
{ "action":"halt" }
```
3. POST /bridge/halt (resume)
```
{ "action":"resume" }
```
4. GET /bridge/state – debug snapshot
```
{
  "halted":false,
  "validators":[{"id":"val1","pubkey":"..."}],
  "custody":{"dyt":"1234"},
  "pending":["id1"],
  "applied":["id2"]
}
```

Behavior:
- If halted, /bridge/ingest rejects with error Internal (future: dedicated code).
- Accepted message immediately increments custody balance and moves to applied set.
- Duplicate submissions return status duplicate (idempotent).
- All activity survives restart.

Logging:
- Rejected ingest prints `bridge_ingest_reject id=... reason=...` to stderr.

Testing Outline:
Unit:
- verify_bridge_message quorum math (1/3, 2/3 thresholds)
- signature validation positive/negative
- halted path returns error
Integration:
1. POST /bridge/ingest with insufficient signatures -> 500 + log reason
2. Sufficient quorum -> accepted, custody increases
3. Halt -> ingest rejected; resume -> ingest works again

Future Enhancements:
- Dedicated error codes (InsufficientQuorum, BridgeHalted)
- Replay protection / nonce per channel
- Outbound mint / burn events
- Rate limiting & fee model for bridge operations
## Validator Keys (Vault or Sealed Keystore)
- No plaintext private keys are ever persisted to disk by the node.
- The node attempts to load validator key material at startup using the following precedence:
  1. Vault (KV v2): set `DYTALLIX_VAULT_URL` and `DYTALLIX_VAULT_TOKEN` (or `VAULT_URL`/`VAULT_TOKEN`). Optional: `DYTALLIX_VAULT_KV_MOUNT` (default `secret`), `DYTALLIX_VAULT_PATH_BASE` (default `dytallix/validators`). Requires `VALIDATOR_ID` (e.g. `val1`).
  2. Sealed local keystore: set `DYT_KEYSTORE_DIR` (default `~/.dytallix/keystore`) and `VALIDATOR_ID`. Provide passphrase via `DYT_KEYSTORE_PASSPHRASE` (non-interactive) or enter when prompted.

Evidence written (redacted):
- `launch-evidence/secrets/vault_config.sample.md`
- `launch-evidence/secrets/keystore_proof.txt` (path, size, sha256 of sealed file)

Key rotation steps:
1. Prepare new key (`newkey.bin`).
2. Vault: `vault kv put secret/dytallix/validators/$VALIDATOR_ID private_key=$(base64 -w0 newkey.bin)`
   Keystore: `DYT_KEYSTORE_PASSPHRASE=...` then run node once; or call provider put function offline.
3. Restart node and verify.
