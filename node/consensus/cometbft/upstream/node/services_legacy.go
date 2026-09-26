//go:build !dytallix_pqc_only

package node

import (
	"fmt"
	cfg "github.com/cometbft/cometbft/config"
	rpccore "github.com/cometbft/cometbft/rpc/core"
	grpccore "github.com/cometbft/cometbft/rpc/grpc"
	rpcserver "github.com/cometbft/cometbft/rpc/jsonrpc/server"
	_ "github.com/lib/pq"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
	"net"
	"net/http"
	_ "net/http/pprof"
	"time"
)

func validateBuildServices(*cfg.Config) error { return nil }
func (n *Node) startGRPCForBuild(env *rpccore.Environment) (net.Listener, error) {
	grpcListenAddr := n.config.RPC.GRPCListenAddress
	config := rpcserver.DefaultConfig()
	config.MaxBodyBytes = n.config.RPC.MaxBodyBytes
	config.MaxHeaderBytes = n.config.RPC.MaxHeaderBytes
	// NOTE: GRPCMaxOpenConnections is used, not MaxOpenConnections
	config.MaxOpenConnections = n.config.RPC.GRPCMaxOpenConnections
	// If necessary adjust global WriteTimeout to ensure it's greater than
	// TimeoutBroadcastTxCommit.
	// See https://github.com/tendermint/tendermint/issues/3435
	if config.WriteTimeout <= n.config.RPC.TimeoutBroadcastTxCommit {
		config.WriteTimeout = n.config.RPC.TimeoutBroadcastTxCommit + 1*time.Second
	}
	listener, err := rpcserver.Listen(grpcListenAddr, config.MaxOpenConnections)
	if err != nil {
		return nil, err
	}
	go func() {
		//nolint:staticcheck // SA1019: core_grpc.StartGRPCClient is deprecated: A new gRPC API will be introduced after v0.38.
		if err := grpccore.StartGRPCServer(env, listener); err != nil {
			n.Logger.Error("Error starting gRPC server", "err", err)
		}
	}()

	return listener, nil
}

// startPrometheusServer starts a Prometheus HTTP server, listening for metrics
// collectors on addr.
func (n *Node) startPrometheusServer() (auxiliaryServer, error) {
	srv := &http.Server{
		Addr: n.config.Instrumentation.PrometheusListenAddr,
		Handler: promhttp.InstrumentMetricHandler(
			prometheus.DefaultRegisterer, promhttp.HandlerFor(
				prometheus.DefaultGatherer,
				promhttp.HandlerOpts{MaxRequestsInFlight: n.config.Instrumentation.MaxOpenConnections},
			),
		),
		ReadHeaderTimeout: readHeaderTimeout,
	}
	go func() {
		if err := srv.ListenAndServe(); err != http.ErrServerClosed {
			// Error starting or closing listener:
			n.Logger.Error("Prometheus HTTP server ListenAndServe", "err", err)
		}
	}()
	return srv, nil
}

// starts a ppro
func (n *Node) startPprofServer() (auxiliaryServer, net.Listener, error) {
	ln, err := net.Listen("tcp", n.config.RPC.PprofListenAddress)
	if err != nil {
		return nil, nil, fmt.Errorf("pprof HTTP server failed to listen: %w", err)
	}
	srv := &http.Server{
		Handler:           nil,
		ReadHeaderTimeout: readHeaderTimeout,
	}
	go func() {
		if err := srv.Serve(ln); err != http.ErrServerClosed {
			n.Logger.Error("pprof HTTP server Serve", "err", err)
		}
	}()
	return srv, ln, nil
}
