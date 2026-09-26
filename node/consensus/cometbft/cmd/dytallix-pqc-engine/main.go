// This command selects explicit experimental PQC transport profiles. It does
// not expose init, reset, remote signing, production, or legacy transport.
package main

import (
	"context"
	ownerguard "dytallix.local/consensus/owner-guard"
	"encoding/json"
	"errors"
	"flag"
	"os"
	"os/signal"
	"syscall"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	"dytallix.local/consensus/cometbft/internal/startupdiag"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/proxy"
	"github.com/cometbft/cometbft/types"
)

func run() error {
	if len(os.Args) < 2 || os.Args[1] != "start" {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("usage: dytallix-pqc-engine start --home HOME --p2p-profile PROFILE [--candidate-staging]")))
	}
	flags := flag.NewFlagSet("start", flag.ContinueOnError)
	home := flags.String("home", "", "existing private fixture home")
	profile := flags.String("p2p-profile", "", "mandatory experimental PQC profile")
	rpcProfile := flags.String("rpc-profile", "", "explicit experimental RPC profile")
	production := flags.Bool("production", false, "production is not supported")
	candidateStaging := flags.Bool("candidate-staging", false, "run the candidate only on a reserved E01 staging chain")
	if err := flags.Parse(os.Args[2:]); err != nil {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, err))
	}
	if flags.NArg() != 0 {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("unexpected positional arguments")))
	}
	if *production {
		return startupdiag.At(1, pqcp2p.RequireProductionTransport())
	}
	var runtime *enginepqc.Runtime
	var err error
	if *candidateStaging {
		if *profile != enginepqc.ProductionCandidateProfile {
			return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("candidate staging requires the production-candidate profile")))
		}
		runtime, err = enginepqc.LoadCandidateForStaging(*home)
	} else {
		runtime, err = enginepqc.Load(*home, *profile)
	}
	if err != nil {
		return startupdiag.At(2, err)
	}
	if err := enginepqc.ConfigureRPCProfile(runtime, *rpcProfile); err != nil {
		return startupdiag.At(3, err)
	}
	logger := log.NewFilter(log.NewTMJSONLogger(log.NewSyncWriter(os.Stdout)), log.AllowInfo())
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()
	engine, err := node.NewNodeWithContext(ctx, runtime.Config, runtime.Validator, runtime.NodeKey,
		proxy.DefaultClientCreator(runtime.Config.ProxyApp, runtime.Config.ABCI, runtime.Config.DBDir()),
		func() (*types.GenesisDoc, error) { return runtime.Genesis, nil }, cfg.DefaultDBProvider,
		node.DefaultMetricsProvider(runtime.Config.Instrumentation), logger,
		node.WithAuthenticatedTransport(runtime.Upgrade(logger)))
	if err != nil {
		return startupdiag.At(4, err)
	}
	summary := runtime.PublicSummary()
	summary["event"] = "experimental_pqc_engine_ready"
	if err = json.NewEncoder(os.Stdout).Encode(summary); err != nil {
		return startupdiag.At(5, err)
	}
	if err = engine.Start(); err != nil {
		return startupdiag.At(6, err)
	}
	<-ctx.Done()
	return startupdiag.At(7, engine.Stop())
}
func main() {
	if err := ownerguard.Run(ownerguard.Engine, run); err != nil {
		os.Exit(startupdiag.ExitCode(err))
	}
}
