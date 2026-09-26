package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/cometbft/cometbft/types"
)

func TestLifecycleFixtureCreatesSixKeysAndFourFundedValidators(t *testing.T) {
	// Short paths are required by the private Unix socket transport.
	root, err := os.MkdirTemp("/tmp", "dyt8-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(root)
	operators := map[string]string{}
	positions := []map[string]string{}
	for i := 0; i < 5; i++ {
		id := fmt.Sprintf("validator-%d", i)
		owner := fmt.Sprintf("owner-%d", i)
		operators[id] = owner
		if i < 4 {
			positions = append(positions, map[string]string{"owner": owner, "validator": id, "amount_udgt": "100"})
		}
	}
	app, _ := json.Marshal(map[string]any{"chain_id": "batch8-test", "reward_v2": map[string]any{"positions": positions}})
	owners, _ := json.Marshal(operators)
	appPath := filepath.Join(root, "app.json")
	operatorsPath := filepath.Join(root, "operators.json")
	if err := os.WriteFile(appPath, app, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(operatorsPath, owners, 0o600); err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(root, "fleet")
	nodes, err := generateWithLifecycle(output, appPath, "batch8-test", "2026-01-01T00:00:00Z", 28650, operatorsPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(nodes) != 6 {
		t.Fatal("wrong node count")
	}
	genesis, err := types.GenesisDocFromFile(filepath.Join(output, "node0", "config", "genesis.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(genesis.Validators) != 4 {
		t.Fatal("candidate keys entered genesis")
	}
	for _, v := range genesis.Validators {
		if v.Power != 100 {
			t.Fatal("power differs from funded principal")
		}
	}
	configRaw, err := os.ReadFile(filepath.Join(output, "application-config.json"))
	if err != nil {
		t.Fatal(err)
	}
	var config applicationConfig
	if err := json.Unmarshal(configRaw, &config); err != nil {
		t.Fatal(err)
	}
	if config.Lifecycle == nil || config.Lifecycle.MinSelfBond != "10" || config.Lifecycle.MaxActive != 8 || config.Profile != config.Lifecycle.Profile {
		t.Fatal("missing explicit lifecycle configuration")
	}
	for i := 0; i < 6; i++ {
		info, err := os.Stat(filepath.Join(output, fmt.Sprintf("node%d", i), "config", "priv_validator_key.json"))
		if err != nil || info.Mode().Perm() != 0o600 {
			t.Fatal("missing private fixture key")
		}
	}
}
