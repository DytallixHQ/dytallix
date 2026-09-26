//go:build dytallix_pqc_only

package server

import (
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
	"net/http"
)

func httpRequestContext(r *http.Request, request *types.RPCRequest) *types.Context {
	return types.NewRequestContext(r.Context(), r.RemoteAddr, request)
}
