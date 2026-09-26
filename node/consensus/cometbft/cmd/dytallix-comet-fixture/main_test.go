package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/types"
)

func TestEmbedGenesisPreservesExactAppBytes(t *testing.T) {
	key, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	params := types.DefaultConsensusParams()
	params.Validator.PubKeyTypes = []string{mldsa65.KeyType}
	doc := &types.GenesisDoc{ChainID: "fixture", InitialHeight: 1, GenesisTime: time.Unix(1, 0), ConsensusParams: params, Validators: []types.GenesisValidator{{PubKey: key.PubKey(), Power: 10}}}
	app := []byte(`{"chain_id":"fixture","nested":{"b":2,"a":"<tag>"}}`)
	encoded, err := embedGenesis(doc, app)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := types.GenesisDocFromJSON(encoded)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(parsed.AppState, app) {
		t.Fatal("application bytes changed")
	}
}

func TestFixtureRejectsNoncompactInputAndExistingDirectory(t *testing.T) {
	dir := t.TempDir()
	input := filepath.Join(dir, "app.json")
	if err := os.WriteFile(input, []byte("{\"chain_id\":\"fixture\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := generate(filepath.Join(dir, "output"), input, "fixture", "2026-01-01T00:00:00Z", 28650); err == nil {
		t.Fatal("accepted noncompact genesis")
	}
	if err := os.WriteFile(input, []byte(`{"chain_id":"fixture"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := generate(dir, input, "fixture", "2026-01-01T00:00:00Z", 28650); err == nil {
		t.Fatal("reused existing output directory")
	}
}
