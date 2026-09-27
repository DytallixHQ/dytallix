// This command selects explicit experimental PQC transport profiles. It does
// not expose init, reset, remote signing, production, or legacy transport.
package main

import (
	"context"
	ownerguard "dytallix.local/consensus/owner-guard"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/lightblocks"
	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	"dytallix.local/consensus/cometbft/internal/startupdiag"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/light"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/proxy"
	"github.com/cometbft/cometbft/types"
)

// exports lists the --light-blocks directories, in order.
type exports []string

func (e *exports) String() string { return fmt.Sprint(*e) }
func (e *exports) Set(dir string) error {
	*e = append(*e, dir)
	return nil
}

// stateSyncOption checks the operator's state sync inputs and builds the
// state provider over their light block exports (Dytallix state sync v1,
// rule 5): the first is the primary, the others witnesses.
func stateSyncOption(ctx context.Context, runtime *enginepqc.Runtime, dirs []string, logger log.Logger) ([]node.Option, error) {
	sync := runtime.Config.StateSync
	if !sync.Enable {
		if len(dirs) != 0 {
			return nil, errors.New("--light-blocks requires state sync to be enabled")
		}
		return nil, nil
	}
	if len(dirs) == 0 {
		return nil, errors.New("state sync requires --light-blocks")
	}
	// A trusted header must still be answerable for evidence.
	if sync.TrustPeriod >= runtime.Genesis.ConsensusParams.Evidence.MaxAgeDuration {
		return nil, errors.New("state sync trust period must be below the evidence age")
	}
	provider, err := lightblocks.NewStateProvider(ctx, runtime.Genesis.ChainID, runtime.Genesis.InitialHeight, dirs,
		light.TrustOptions{Period: sync.TrustPeriod, Height: sync.TrustHeight, Hash: sync.TrustHashBytes()},
		logger.With("module", "light"))
	if err != nil {
		return nil, err
	}
	return []node.Option{node.StateProvider(provider)}, nil
}

func run() error {
	if len(os.Args) < 2 || os.Args[1] != "start" {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("usage: dytallix-pqc-engine start --home HOME --p2p-profile PROFILE [--candidate-staging] [--light-blocks DIR]...")))
	}
	flags := flag.NewFlagSet("start", flag.ContinueOnError)
	home := flags.String("home", "", "existing private fixture home")
	profile := flags.String("p2p-profile", "", "mandatory experimental PQC profile")
	rpcProfile := flags.String("rpc-profile", "", "explicit experimental RPC profile")
	production := flags.Bool("production", false, "production is not supported")
	candidateStaging := flags.Bool("candidate-staging", false, "run the candidate only on a reserved E01 staging chain")
	var lightBlocks exports
	flags.Var(&lightBlocks, "light-blocks", "operator light block export for state sync; repeat for witnesses")
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
	options, err := stateSyncOption(ctx, runtime, lightBlocks, logger)
	if err != nil {
		return startupdiag.At(3, err)
	}
	options = append(options, node.WithAuthenticatedTransport(runtime.Upgrade(logger)))
	engine, err := node.NewNodeWithContext(ctx, runtime.Config, runtime.Validator, runtime.NodeKey,
		proxy.DefaultClientCreator(runtime.Config.ProxyApp, runtime.Config.ABCI, runtime.Config.DBDir()),
		func() (*types.GenesisDoc, error) { return runtime.Genesis, nil }, cfg.DefaultDBProvider,
		node.DefaultMetricsProvider(runtime.Config.Instrumentation), logger, options...)
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
