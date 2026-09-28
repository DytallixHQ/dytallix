//go:build dytallix_pqc_ipc && dytallix_pqc_only

package node

import (
	"errors"
	"github.com/cometbft/cometbft/rpc/jsonrpc/ipc"
	"net"
	"path/filepath"
)

// startRPC serves the client allowlist on HOME/data/rpc.sock and adds the
// operator methods on HOME/data/rpc-operator.sock (RPC controls v1). Both
// sockets are mode 0600 in a 0700 directory.
func (n *Node) startRPC() ([]net.Listener, error) {
	data := filepath.Join(n.config.RootDir, "data")
	expected := filepath.Join(data, ipc.ClientSocket)
	if n.config.RPC.ListenAddress != "unix://"+expected || n.config.RPC.Unsafe {
		return nil, errors.New("explicit private Unix RPC profile required")
	}
	environment, err := n.ConfigureRPC()
	if err != nil {
		return nil, err
	}
	return ipc.ServeSockets(data, environment.GetRoutes(), n.config.RPC.MaxRequestBatchSize)
}
