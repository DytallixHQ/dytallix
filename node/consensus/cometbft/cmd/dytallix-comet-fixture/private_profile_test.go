package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
)

func TestPrivateSeedFixtureRequiresExplicitDistinctHostIPs(t *testing.T) {
	root, err := os.MkdirTemp("/tmp", "pqc-private-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"private-fixture"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	valid := []string{"10.1.2.1", "10.1.2.2", "10.1.2.3", "10.1.2.4"}
	for name, ips := range map[string][]string{
		"missing":   nil,
		"duplicate": {"10.1.2.1", "10.1.2.1", "10.1.2.3", "10.1.2.4"},
		"public":    {"10.1.2.1", "203.0.113.1", "10.1.2.3", "10.1.2.4"},
		"loopback":  {"10.1.2.1", "127.0.0.1", "10.1.2.3", "10.1.2.4"},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := generateWithTransportIPs(filepath.Join(root, name), app, "private-fixture", "2026-01-01T00:00:00Z", 28650, "", false, pqcPrivateSeedTransport, ips); err == nil {
				t.Fatal("unapproved private topology accepted")
			}
		})
	}
	if _, err := generateWithTransportIPs(filepath.Join(root, "mainnet"), app, "private-mainnet", "2026-01-01T00:00:00Z", 28650, "", false, pqcPrivateSeedTransport, valid); err == nil {
		t.Fatal("mainnet chain ID accepted in staging profile")
	}
	output := filepath.Join(root, "nodes")
	nodes, err := generateWithTransportIPs(output, app, "private-fixture", "2026-01-01T00:00:00Z", 28650, "", false, pqcPrivateSeedTransport, valid)
	if err != nil {
		t.Fatal(err)
	}
	for _, node := range nodes {
		if _, err := os.Lstat(filepath.Join(node.Home, "config", "node_key.json")); !os.IsNotExist(err) {
			t.Fatalf("packed peer key exists: %v", err)
		}
		runtime, err := enginepqc.Load(node.Home, enginepqc.RemoteSeedProfile)
		if enginepqc.BuildProfile != "dytallix_pqc_only" || enginepqc.RPCBuildProfile != "dytallix-pqc-unix-v1" {
			if err == nil {
				t.Fatal("private profile accepted a nonselected build")
			}
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		if runtime.PublicSummary()["profile"] != enginepqc.RemoteSeedProfile {
			t.Fatal("runtime profile mismatch")
		}
		restarted, err := enginepqc.Load(node.Home, enginepqc.RemoteSeedProfile)
		if err != nil {
			t.Fatal(err)
		}
		if runtime.NodeKey.ID() != restarted.NodeKey.ID() ||
			!bytes.Equal(runtime.NodeKey.PubKey().Bytes(), restarted.NodeKey.PubKey().Bytes()) {
			t.Fatal("seed-backed peer identity changed after runtime reload")
		}
	}
}
