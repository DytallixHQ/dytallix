package ipc

import (
	"fmt"
	"net"
	"path/filepath"
	"sort"

	"github.com/cometbft/cometbft/rpc/jsonrpc/dispatch"
)

// Method allowlists (RPC controls v1, E04 gap 10; P01 28 September 2026).
// The client socket, rpc.sock, serves ClientMethods; it is what the loopback
// HTTP adapter, and through it any gateway, reaches. The operator socket,
// rpc-operator.sock, also serves OperatorMethods, which reveal peers, the
// mempool and consensus internals. Every other route, including tx_search,
// block_search, unconfirmed_txs, broadcast_tx_commit, broadcast_tx_async and
// the WebSocket subscriptions, is served on neither socket.
var ClientMethods = []string{
	"abci_info",
	"abci_query",
	"block",
	"block_by_hash",
	"block_results",
	"blockchain",
	"broadcast_evidence",
	"broadcast_tx_sync",
	"check_tx",
	"commit",
	"consensus_params",
	"genesis_chunked",
	"header",
	"header_by_hash",
	"health",
	"status",
	"tx",
	"validators",
}

// OperatorMethods are served on the operator socket only.
var OperatorMethods = []string{
	"consensus_state",
	"dump_consensus_state",
	"net_info",
	"num_unconfirmed_txs",
}

// ClientSocket and OperatorSocket are the socket names under HOME/data.
const (
	ClientSocket   = "rpc.sock"
	OperatorSocket = "rpc-operator.sock"
)

// Select returns exactly the named routes. A name the route map lacks is an
// error, so a renamed upstream route cannot silently leave the allowlist.
func Select[T any](routes map[string]T, lists ...[]string) (map[string]T, error) {
	selected := make(map[string]T)
	var missing []string
	for _, list := range lists {
		for _, name := range list {
			route, ok := routes[name]
			if !ok {
				missing = append(missing, name)
				continue
			}
			selected[name] = route
		}
	}
	if len(missing) > 0 {
		sort.Strings(missing)
		return nil, fmt.Errorf("allowlisted RPC methods missing from the route map: %v", missing)
	}
	return selected, nil
}

// ServeSockets serves the client allowlist on data/rpc.sock and the client
// and operator methods on data/rpc-operator.sock. data must be a private
// directory (mode 0700); both sockets are created mode 0600.
func ServeSockets(data string, routes map[string]*dispatch.RPCFunc, maxBatch int) ([]net.Listener, error) {
	client, err := Select(routes, ClientMethods)
	if err != nil {
		return nil, err
	}
	operator, err := Select(routes, ClientMethods, OperatorMethods)
	if err != nil {
		return nil, err
	}
	clientListener, err := Listen(filepath.Join(data, ClientSocket), NewHandler(client, maxBatch))
	if err != nil {
		return nil, err
	}
	operatorListener, err := Listen(filepath.Join(data, OperatorSocket), NewHandler(operator, maxBatch))
	if err != nil {
		_ = clientListener.Close()
		return nil, err
	}
	return []net.Listener{clientListener, operatorListener}, nil
}
