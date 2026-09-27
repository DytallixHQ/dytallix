package main

import (
	"context"
	"strings"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/types"
)

// State sync and light block exports go together, and the trust period stays
// below the evidence age.
func TestStateSyncNeedsLightBlocksAndABoundedTrustPeriod(t *testing.T) {
	runtime := &enginepqc.Runtime{Config: cfg.DefaultConfig(), Genesis: &types.GenesisDoc{
		ChainID: "c", InitialHeight: 1, ConsensusParams: types.DefaultConsensusParams(),
	}}
	ctx, logger := context.Background(), log.NewNopLogger()
	runtime.Config.StateSync.Enable = false
	if options, err := stateSyncOption(ctx, runtime, nil, logger); err != nil || options != nil {
		t.Fatal(options, err)
	}
	if _, err := stateSyncOption(ctx, runtime, []string{t.TempDir()}, logger); err == nil {
		t.Fatal("light blocks accepted without state sync")
	}
	runtime.Config.StateSync.Enable = true
	if _, err := stateSyncOption(ctx, runtime, nil, logger); err == nil {
		t.Fatal("state sync accepted without light blocks")
	}
	runtime.Config.StateSync.TrustPeriod = runtime.Genesis.ConsensusParams.Evidence.MaxAgeDuration
	_, err := stateSyncOption(ctx, runtime, []string{t.TempDir()}, logger)
	if err == nil || !strings.Contains(err.Error(), "below the evidence age") {
		t.Fatal(err)
	}
	// Within the bound, the light client needs the trusted header.
	runtime.Config.StateSync.TrustPeriod = time.Hour
	runtime.Config.StateSync.TrustHeight = 3
	runtime.Config.StateSync.TrustHash = strings.Repeat("ab", 32)
	if _, err = stateSyncOption(ctx, runtime, []string{t.TempDir()}, logger); err == nil {
		t.Fatal("state provider built without the trusted header")
	}
}
