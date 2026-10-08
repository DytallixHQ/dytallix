// This command selects explicit PQC transport profiles: the development and
// staging profiles in a development build, and only the production profile,
// with its binding, in a production build. It does not expose init, reset,
// remote signing or legacy transport.
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
	"path/filepath"
	"syscall"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/lightblocks"
	"dytallix.local/consensus/cometbft/internal/metricsfile"
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
	// The chunk queue goes under the node's own data, a writable root, unless
	// the config names a directory.
	if sync.TempDir == "" {
		sync.TempDir = runtime.Config.DBDir()
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

// metricsOption builds the metrics provider and file writer for an operator
// directory and interval (metrics v1), or the default no-op provider when
// neither is set.
func metricsOption(config *cfg.Config, dir string, interval time.Duration) (node.MetricsProvider, func(context.Context), error) {
	if dir == "" && interval == 0 {
		return node.DefaultMetricsProvider(config.Instrumentation), func(context.Context) {}, nil
	}
	if dir == "" || !filepath.IsAbs(dir) || interval < time.Second || interval > time.Hour {
		return nil, nil, errors.New("--metrics-dir (absolute) and --metrics-interval (1s to 1h) go together")
	}
	if stat, err := os.Stat(dir); err != nil || !stat.IsDir() {
		return nil, nil, errors.New("--metrics-dir must be an existing directory")
	}
	registry := metricsfile.NewRegistry()
	write := func(ctx context.Context) {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			// A failed write leaves the previous file, which then shows as stale.
			_ = registry.WriteFile(dir, "dytallix-engine.prom", "engine", time.Now())
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
			}
		}
	}
	return metricsfile.Provider(registry), write, nil
}

func run() error {
	if len(os.Args) < 2 || os.Args[1] != "start" {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("usage: dytallix-pqc-engine start --home HOME --p2p-profile PROFILE [--binding FILE] [--candidate-staging] [--light-blocks DIR]... [--restore-snapshot DIR] [--snapshot-light-blocks DIR --snapshot-interval N --snapshot-keep K]")))
	}
	flags := flag.NewFlagSet("start", flag.ContinueOnError)
	home := flags.String("home", "", "existing private fixture home")
	profile := flags.String("p2p-profile", "", "mandatory experimental PQC profile")
	rpcProfile := flags.String("rpc-profile", "", "explicit experimental RPC profile")
	production := flags.Bool("production", false, "production is not supported")
	candidateStaging := flags.Bool("candidate-staging", false, "run the candidate only on a reserved E01 staging chain")
	bindingPath := flags.String("binding", "", "this host's production binding (the production profile only)")
	var lightBlocks exports
	flags.Var(&lightBlocks, "light-blocks", "operator light block export for state sync; repeat for witnesses")
	metricsDir := flags.String("metrics-dir", "", "directory for the dytallix-engine.prom metrics file")
	metricsInterval := flags.Duration("metrics-interval", 0, "metrics file write interval")
	snapshotLightBlocks := flags.String("snapshot-light-blocks", "", "directory for the light blocks of each application snapshot (the sentry)")
	snapshotInterval := flags.Int64("snapshot-interval", 0, "the application's snapshot interval in blocks")
	snapshotKeep := flags.Int("snapshot-keep", 0, "the number of snapshots the application keeps")
	restoreSnapshot := flags.String("restore-snapshot", "", "restore this node, which holds no state, from an opened off-host copy")
	if err := flags.Parse(os.Args[2:]); err != nil {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, err))
	}
	if flags.NArg() != 0 {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("unexpected positional arguments")))
	}
	if *production {
		return startupdiag.At(1, pqcp2p.RequireProductionTransport())
	}
	if (*snapshotLightBlocks != "") != (*snapshotInterval != 0) || (*snapshotLightBlocks != "") != (*snapshotKeep != 0) {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("--snapshot-light-blocks, --snapshot-interval and --snapshot-keep go together")))
	}
	var runtime *enginepqc.Runtime
	var err error
	if *candidateStaging {
		if enginepqc.ProductionBuild {
			return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("a production build has no candidate staging mode")))
		}
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
	// The production profile starts only with this host's published binding
	// (production activation v1, A4); no other profile takes one.
	if (*profile == enginepqc.ProductionProfile) != (*bindingPath != "") {
		return startupdiag.At(2, startupdiag.AsClass(startupdiag.Invalid, errors.New("--binding goes with the production profile, which requires it")))
	}
	var signer types.PrivValidator = runtime.Validator
	if *bindingPath != "" {
		binding, err := enginepqc.LoadProductionBinding(*bindingPath)
		if err != nil {
			return startupdiag.At(2, err)
		}
		if err := enginepqc.ValidateProductionBinding(runtime, binding); err != nil {
			return startupdiag.At(2, err)
		}
		// A sentry or endpoint never signs (P01, 3 October 2026).
		signer = enginepqc.SignerForRole(runtime, binding.Role)
	}
	if err := enginepqc.ConfigureRPCProfile(runtime, *rpcProfile); err != nil {
		return startupdiag.At(3, err)
	}
	logger := log.NewFilter(log.NewTMJSONLogger(log.NewSyncWriter(os.Stdout)), log.AllowInfo())
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()
	// Disaster recovery v1, R5: a one-time restore from an opened copy, with
	// the copy's own light blocks.
	var restore []node.Option
	if *restoreSnapshot != "" {
		if len(lightBlocks) != 0 {
			return startupdiag.At(3, startupdiag.AsClass(startupdiag.Invalid, errors.New("--restore-snapshot takes its light blocks from the copy")))
		}
		local, light, err := restoreOption(ctx, runtime, *restoreSnapshot)
		if err != nil {
			return startupdiag.At(3, err)
		}
		lightBlocks = exports{light}
		restore = []node.Option{node.LocalSnapshot(local)}
	}
	options, err := stateSyncOption(ctx, runtime, lightBlocks, logger)
	if err != nil {
		return startupdiag.At(3, err)
	}
	options = append(options, restore...)
	metrics, writeMetrics, err := metricsOption(runtime.Config, *metricsDir, *metricsInterval)
	if err != nil {
		return startupdiag.At(3, err)
	}
	options = append(options, node.WithAuthenticatedTransport(runtime.Upgrade(logger)))
	engine, err := node.NewNodeWithContext(ctx, runtime.Config, signer, runtime.NodeKey,
		proxy.DefaultClientCreator(runtime.Config.ProxyApp, runtime.Config.ABCI, runtime.Config.DBDir()),
		func() (*types.GenesisDoc, error) { return runtime.Genesis, nil }, cfg.DefaultDBProvider,
		metrics, logger, options...)
	if err != nil {
		return startupdiag.At(4, err)
	}
	// Disaster recovery v1, R4: the light blocks each snapshot's restore
	// verifies, written beside the application's snapshots.
	var snapshotWriter *lightblocks.SnapshotWriter
	if *snapshotLightBlocks != "" {
		snapshotWriter, err = lightblocks.NewSnapshotWriter(*snapshotLightBlocks, *snapshotInterval, *snapshotKeep,
			runtime.Genesis.ChainID, engine.BlockStore(), engine.StateStore())
		if err != nil {
			return startupdiag.At(4, err)
		}
	}
	summary := runtime.PublicSummary()
	summary["event"] = "experimental_pqc_engine_ready"
	summary["version"] = readyVersion
	if err = json.NewEncoder(os.Stdout).Encode(summary); err != nil {
		return startupdiag.At(5, err)
	}
	if err = engine.Start(); err != nil {
		return startupdiag.At(6, err)
	}
	go writeMetrics(ctx)
	if snapshotWriter != nil {
		go snapshotWriter.Run(ctx, time.Second, logger.With("module", "snapshot-light-blocks"))
	}
	<-ctx.Done()
	return startupdiag.At(7, engine.Stop())
}

// readyVersion versions the ready line printed to stdout (interfaces v1).
const readyVersion = 1

func main() {
	if err := ownerguard.Run(ownerguard.Engine, run); err != nil {
		os.Exit(startupdiag.ExitCode(err))
	}
}
