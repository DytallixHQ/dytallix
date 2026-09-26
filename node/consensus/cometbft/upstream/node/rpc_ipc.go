//go:build dytallix_pqc_ipc && dytallix_pqc_only

package node

import (
	"errors"
	"github.com/cometbft/cometbft/rpc/jsonrpc/ipc"
	"net"
	"path/filepath"
)

func (n *Node) startRPC() ([]net.Listener, error) {
	expected := filepath.Join(n.config.RootDir, "data", "rpc.sock")
	if n.config.RPC.ListenAddress != "unix://"+expected || n.config.RPC.Unsafe {
		return nil, errors.New("explicit private Unix RPC profile required")
	}
	environment, err := n.ConfigureRPC()
	if err != nil {
		return nil, err
	}
	listener, err := ipc.Listen(expected, ipc.NewHandler(environment.GetRoutes(), n.config.RPC.MaxRequestBatchSize))
	if err != nil {
		return nil, err
	}
	return []net.Listener{listener}, nil
}
