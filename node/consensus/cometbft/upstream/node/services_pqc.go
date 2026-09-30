package node

import (
	"errors"
	cfg "github.com/cometbft/cometbft/config"
	rpccore "github.com/cometbft/cometbft/rpc/core"
	"net"
)

func validateBuildServices(c *cfg.Config) error {
	// State sync runs only with a state provider the engine injects; the
	// HTTP light client is not built (Dytallix state sync v1, rule 5).
	if c.RPC.GRPCListenAddress != "" || c.ABCI != "socket" || c.RPC.IsPprofEnabled() || c.Instrumentation.IsPrometheusEnabled() || c.RPC.IsTLSEnabled() || c.TxIndex.Indexer == "psql" {
		return errors.New("the Dytallix PQC-only fork has no gRPC, RPC TLS, profiling, Prometheus listener or SQL indexer")
	}
	return nil
}
func (n *Node) startGRPCForBuild(*rpccore.Environment) (net.Listener, error) {
	return nil, errors.New("gRPC RPC is not built (Dytallix PQC-only fork)")
}
func (n *Node) startPprofServer() (auxiliaryServer, net.Listener, error) {
	return nil, nil, errors.New("profiling is not built (Dytallix PQC-only fork)")
}

// validateBuildServices rejects this configuration before node state is opened.
func (n *Node) startPrometheusServer() (auxiliaryServer, error) {
	return nil, errors.New("Prometheus listener is not built (Dytallix PQC-only fork)")
}
