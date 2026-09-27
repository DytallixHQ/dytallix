package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	cfg "github.com/cometbft/cometbft/config"
)

// E04 gap 6: the engine keeps the flood mempool with recheck, which the
// application's per-head admission queue depends on, and refuses a mempool
// transaction limit above the genesis block size. Fixture homes pass.
func TestEngineRefusesMempoolSettingsTheAdmissionQueueCannotHold(t *testing.T) {
	root, err := os.MkdirTemp("/tmp", "pqc-mempool-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"mempool-fixture"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	nodes, err := generateWithTransport(filepath.Join(root, "nodes"), app, "mempool-fixture",
		"2026-01-01T00:00:00Z", 28650, "", false, pqcLoopbackTransport)
	if err != nil {
		t.Fatal(err)
	}
	home := nodes[0].Home
	if _, err := enginepqc.Load(home, enginepqc.Profile); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, "config", "config.toml")
	original, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for name, change := range map[string]func(*cfg.Config){
		"app mempool":      func(c *cfg.Config) { c.Mempool.Type = cfg.MempoolTypeApp },
		"nop mempool":      func(c *cfg.Config) { c.Mempool.Type = cfg.MempoolTypeNop },
		"recheck off":      func(c *cfg.Config) { c.Mempool.Recheck = false },
		"oversized tx cap": func(c *cfg.Config) { c.Mempool.MaxTxBytes = 2 << 20 },
	} {
		t.Run(name, func(t *testing.T) {
			c, err := enginepqc.ReadConfig(home)
			if err != nil {
				t.Fatal(err)
			}
			change(c)
			cfg.WriteConfigFile(path, c)
			if err := os.Chmod(path, 0o600); err != nil {
				t.Fatal(err)
			}
			_, err = enginepqc.Load(home, enginepqc.Profile)
			if err == nil || !strings.Contains(err.Error(), "mempool") {
				t.Fatalf("accepted: %v", err)
			}
			if err := os.WriteFile(path, original, 0o600); err != nil {
				t.Fatal(err)
			}
		})
	}
}
