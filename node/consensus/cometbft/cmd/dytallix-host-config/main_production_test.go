//go:build production

package main

import (
	"crypto/sha256"
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/types"
)

// seedKey is a disposable test key from a labeled seed.
func seedKey(t *testing.T, kind, label string) ([]byte, mldsa65.PrivKey) {
	t.Helper()
	seed := sha256.Sum256([]byte("dytallix-host-config-test/" + kind + "/" + label))
	key, err := mldsa65.GenPrivKeyFromSeed(seed[:])
	if err != nil {
		t.Fatal(err)
	}
	return seed[:], key
}

// A host that installs its generated files with its own seed and validator
// key starts on the production profile, and the binding the engine computes
// from them is the one the pin plan publishes.
func TestPublishedBindingsMatchTheEngineAtStart(t *testing.T) {
	root := t.TempDir()
	genesis := &types.GenesisDoc{ChainID: "dytallix-host-check-1", InitialHeight: 1,
		GenesisTime: time.Date(2026, 10, 3, 0, 0, 0, 0, time.UTC), ConsensusParams: types.DefaultConsensusParams()}
	genesis.ConsensusParams.Block.MaxBytes = 1 << 20
	genesis.ConsensusParams.Evidence.MaxBytes = 65536
	genesis.ConsensusParams.Evidence.MaxAgeDuration = 336 * time.Hour
	genesis.ConsensusParams.Validator.PubKeyTypes = []string{mldsa65.KeyType}
	for _, label := range []string{"validator-1", "validator-2"} {
		_, key := seedKey(t, "validator", label)
		genesis.Validators = append(genesis.Validators, types.GenesisValidator{PubKey: key.PubKey(), Power: 10, Name: label})
	}
	if err := genesis.ValidateAndComplete(); err != nil {
		t.Fatal(err)
	}
	genesisRaw, err := cmtjson.MarshalIndent(genesis, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	channel := "203.0.113.31:9443"
	hosts := []host{
		{Label: "validator-1", Operator: "operator-1", Role: "validator", P2P: "192.0.2.31:26656", Pins: []string{"sentry-1"}},
		{Label: "validator-2", Operator: "operator-2", Role: "validator", P2P: "192.0.2.32:26656", Pins: []string{"sentry-2"}},
		{Label: "sentry-1", Operator: "operator-1", Role: "sentry", P2P: "198.51.100.31:26656", Pins: []string{"validator-1", "sentry-2", "endpoint-1"}},
		{Label: "sentry-2", Operator: "operator-2", Role: "sentry", P2P: "198.51.100.32:26656", Pins: []string{"validator-2", "sentry-1", "endpoint-1"}},
		{Label: "endpoint-1", Operator: "operator-1", Role: "endpoint", P2P: "203.0.113.31:26656", Channel: &channel, Pins: []string{"sentry-1", "sentry-2"}},
	}
	seeds := map[string][]byte{}
	validators := map[string]mldsa65.PrivKey{}
	for i := range hosts {
		h := &hosts[i]
		h.Home = filepath.Join(root, h.Label)
		seed, peer := seedKey(t, "peer", h.Label)
		_, validator := seedKey(t, "validator", h.Label)
		seeds[h.Label], validators[h.Label] = seed, validator
		h.PeerPublicKey = base64.StdEncoding.EncodeToString(peer.PubKey().Bytes())
		h.ValidatorPublicKey = base64.StdEncoding.EncodeToString(validator.PubKey().Bytes())
	}
	files, err := generate(encodePlan(t, plan{Schema: planSchema, ChainID: genesis.ChainID, Hosts: hosts}),
		read(t, filepath.Join(rehearsal, "host-values.json")), genesisRaw, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	byPath := filesByPath(files)
	for _, h := range hosts {
		for _, dir := range []string{"", "config", "data", "abci"} {
			if err := os.MkdirAll(filepath.Join(h.Home, dir), 0o700); err != nil {
				t.Fatal(err)
			}
		}
		generated := filepath.Join("hosts", h.Label)
		for name, data := range map[string][]byte{
			"config.toml":          byPath[filepath.Join(generated, "config.toml")],
			"pqc_transport.json":   byPath[filepath.Join(generated, "pqc_transport.json")],
			"binding.json":         byPath[filepath.Join(generated, "binding.json")],
			"genesis.json":         genesisRaw,
			enginepqc.SeedFileName: seeds[h.Label],
		} {
			if err := os.WriteFile(filepath.Join(h.Home, "config", name), data, 0o600); err != nil {
				t.Fatal(err)
			}
		}
		privval.NewFilePV(validators[h.Label], filepath.Join(h.Home, "config", "priv_validator_key.json"),
			filepath.Join(h.Home, "data", "priv_validator_state.json")).Save()
		runtime, err := enginepqc.Load(h.Home, enginepqc.ProductionProfile)
		if err != nil {
			t.Fatalf("%s: the engine refused its generated files: %v", h.Label, err)
		}
		published, err := enginepqc.LoadProductionBinding(filepath.Join(h.Home, "config", "binding.json"))
		if err != nil {
			t.Fatal(err)
		}
		computed, err := enginepqc.NewProductionBinding(runtime, h.Role)
		if err != nil || computed != published {
			t.Fatalf("%s: the engine's binding differs from the published one: %v", h.Label, err)
		}
	}
}
