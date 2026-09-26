# PulseGuard API (Draft)

[Docs hub](../../../../../docs/README.md) | [RPC and API docs](../../../../../docs/rpc-and-apis.md)

POST /pulseguard/score
Request: { "tx_hash": "...", "snapshot": false, "details": false }
Response: { "tx_hash":"...", "score": 0-100, "confidence":0-1, "reasons":[], "explainability": { "top_features": [[name,val]], "paths": [] }, "p95_budget_ms":100 }
Headers: x-pqc-algo, x-pqc-sig, x-evidence-sha256

GET /pulseguard/stream (SSE/WS planned) -> prioritized alerts.
