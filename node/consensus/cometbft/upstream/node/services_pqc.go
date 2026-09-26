//go:build dytallix_pqc_only

package node

import (
	"errors"
	cfg "github.com/cometbft/cometbft/config"
	rpccore "github.com/cometbft/cometbft/rpc/core"
	"net"
)

func validateBuildServices(c *cfg.Config) error {
	if c.StateSync.Enable || c.RPC.GRPCListenAddress != "" || c.ABCI != "socket" || c.RPC.IsPprofEnabled() || c.Instrumentation.IsPrometheusEnabled() || c.RPC.IsTLSEnabled() || c.TxIndex.Indexer == "psql" {
		return errors.New("dytallix_pqc_only excludes state sync, gRPC, RPC TLS, profiling, Prometheus listener and SQL indexer")
	}
	return nil
}
func (n *Node) startGRPCForBuild(*rpccore.Environment) (net.Listener, error) {
	return nil, errors.New("gRPC RPC excluded by dytallix_pqc_only")
}
func (n *Node) startPprofServer() (auxiliaryServer, net.Listener, error) {
	return nil, nil, errors.New("profiling excluded by dytallix_pqc_only")
}

// validateBuildServices rejects this configuration before node state is opened.
func (n *Node) startPrometheusServer() (auxiliaryServer, error) {
	return nil, errors.New("Prometheus listener excluded by dytallix_pqc_only")
}
