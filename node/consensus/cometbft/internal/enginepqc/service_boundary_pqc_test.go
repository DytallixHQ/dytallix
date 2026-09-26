//go:build dytallix_pqc_only

package enginepqc

import (
	"context"
	"strings"
	"testing"

	abcicli "github.com/cometbft/cometbft/abci/client"
	abciserver "github.com/cometbft/cometbft/abci/server"
	abcitypes "github.com/cometbft/cometbft/abci/types"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/node"
	rpcserver "github.com/cometbft/cometbft/rpc/jsonrpc/server"
	"github.com/cometbft/cometbft/state/indexer/block"
)

func TestPQCBuildServicesRejectBeforeDatabaseOpen(t *testing.T) {
	cases := map[string]func(*cfg.Config){
		"grpc rpc":   func(c *cfg.Config) { c.RPC.GRPCListenAddress = "tcp://127.0.0.1:1" },
		"grpc abci":  func(c *cfg.Config) { c.ABCI = "grpc" },
		"profiling":  func(c *cfg.Config) { c.RPC.PprofListenAddress = "127.0.0.1:1" },
		"prometheus": func(c *cfg.Config) { c.Instrumentation.Prometheus = true },
		"rpc tls":    func(c *cfg.Config) { c.RPC.TLSCertFile = "unopened-cert"; c.RPC.TLSKeyFile = "unopened-key" },
		"sql":        func(c *cfg.Config) { c.TxIndex.Indexer = "psql" },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			config := isolatedConfig(t)
			change(config)
			// Nil providers would panic if construction proceeded to database or application work.
			engine, err := node.NewNodeWithContext(context.Background(), config, nil, nil, nil, nil, nil, nil, log.NewNopLogger())
			if engine != nil || err == nil || !strings.Contains(err.Error(), "excludes") {
				t.Fatalf("unsupported service reached construction: %v", err)
			}
		})
	}
}

func TestPQCBuildGRPCABCIExcluded(t *testing.T) {
	client, err := abcicli.NewClient("127.0.0.1:1", "grpc", true)
	if client != nil || err == nil || !strings.Contains(err.Error(), "excluded") {
		t.Fatalf("gRPC client accepted: %v", err)
	}
	socket, err := abcicli.NewClient("unix:///unopened.sock", "socket", false)
	if socket == nil || err != nil {
		t.Fatalf("socket ABCI constructor changed: %v", err)
	}
}

func TestPQCBuildSQLIndexerExcluded(t *testing.T) {
	config := isolatedConfig(t)
	config.TxIndex.Indexer = "psql"
	txidx, blockidx, err := block.IndexerFromConfig(config, nil, "local-fixture")
	if txidx != nil || blockidx != nil || err == nil || !strings.Contains(err.Error(), "excluded") {
		t.Fatalf("SQL indexer accepted: %v", err)
	}
}

func TestPQCBuildRPCServeTLSExcluded(t *testing.T) {
	// Nil arguments prove the disabled adapter fails before listener or key access.
	if err := rpcserver.ServeTLS(nil, nil, "unopened-cert", "unopened-key", nil, nil); err == nil || !strings.Contains(err.Error(), "excluded") {
		t.Fatalf("RPC TLS adapter accepted: %v", err)
	}
}

func TestPQCBuildGRPCABCIServerExcluded(t *testing.T) {
	server, err := abciserver.NewServer("tcp://127.0.0.1:1", "grpc", abcitypes.NewBaseApplication())
	if server != nil || err == nil || !strings.Contains(err.Error(), "excluded") {
		t.Fatalf("gRPC ABCI server accepted: %v", err)
	}
}
