//go:build !production

package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/types"
)

// The genesis rehearsal (E05-d) is written by the Rust genesis builder. The
// engine must decode it and re-encode exactly the same bytes, with the native
// genesis carried unchanged as app_state, and it must pass the engine's own
// genesis checks.
const rehearsalDir = "../../../../tools/mainnet-preparation/fixtures/genesis-rehearsal"

func TestRehearsalGenesisIsTheEngineEncoding(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join(rehearsalDir, "genesis.json"))
	if err != nil {
		t.Fatal(err)
	}
	app, err := os.ReadFile(filepath.Join(rehearsalDir, "native-genesis.json"))
	if err != nil {
		t.Fatal(err)
	}
	doc, err := types.GenesisDocFromJSON(raw)
	if err != nil {
		t.Fatal(err)
	}
	again, err := embedGenesis(doc, app)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(again, raw) {
		t.Fatalf("the engine re-encodes the rehearsal genesis differently:\n%s\n%s", again[:min(len(again), 600)], raw[:min(len(raw), 600)])
	}
	if !bytes.Equal(doc.AppState, app) {
		t.Fatal("app_state is not the exact native genesis")
	}

	// The engine loader's checks (internal/enginepqc/engine.go).
	lower := strings.ToLower(doc.ChainID)
	if doc.ChainID == "" || len(doc.ChainID) > 50 || strings.Contains(lower, "mainnet") || strings.Contains(lower, "production") {
		t.Fatalf("chain ID %q is refused by the engine", doc.ChainID)
	}
	if doc.InitialHeight != 1 || len(doc.Validators) < 1 || len(doc.Validators) > 64 {
		t.Fatal("the engine needs initial height 1 and 1 to 64 validators")
	}
	if doc.GenesisTime.IsZero() || doc.GenesisTime.Nanosecond() != 0 || doc.GenesisTime.Location() != time.UTC {
		t.Fatal("genesis time must be explicit whole seconds in UTC")
	}
	params := doc.ConsensusParams
	if len(params.Validator.PubKeyTypes) != 1 || params.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		t.Fatal("genesis must select only ML-DSA-65 validator keys")
	}
	if params.Evidence.MaxAgeDuration%time.Second != 0 {
		t.Fatal("the application compares the evidence age in whole seconds")
	}
	if params.ABCI.VoteExtensionsEnableHeight != 0 {
		t.Fatal("the bridge refuses vote extensions")
	}
	seen := map[string]bool{}
	var total int64
	for _, v := range doc.Validators {
		if v.PubKey == nil || v.PubKey.Type() != mldsa65.KeyType || len(v.PubKey.Bytes()) != mldsa65.PubKeySize || v.Power <= 0 {
			t.Fatalf("unsupported validator %s", v.Name)
		}
		if !bytes.Equal(v.Address, v.PubKey.Address()) || seen[v.Address.String()] {
			t.Fatalf("validator %s has a wrong or repeated address", v.Name)
		}
		seen[v.Address.String()] = true
		total += v.Power
		if total > types.MaxTotalVotingPower {
			t.Fatal("total voting power exceeds the engine bound")
		}
	}
}
