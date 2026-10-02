//go:build !production

package enginepqc

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// The engine loads a genesis carrying the 8 MiB application genesis the rest
// of the stack allows, and refuses one beyond its bound (production
// activation v1, A6).
func TestEngineLoadsAGenesisCarryingAnEightMiBApplicationGenesis(t *testing.T) {
	root, err := os.MkdirTemp("/tmp", "a6g-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"dytallix-genesis-size-1"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	fleet := filepath.Join(root, "fleet")
	cmd := exec.Command("go", "run", "./cmd/dytallix-comet-fixture", "--output", fleet, "--app-genesis", app,
		"--chain-id", "dytallix-genesis-size-1", "--genesis-time", "2026-10-02T00:00:00Z", "--base-port", "39850",
		"--p2p-profile", Profile)
	cmd.Dir = filepath.Join("..", "..")
	cmd.Env = append(os.Environ(), "GOPROXY=off", "GOSUMDB=off", "CGO_ENABLED=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("disposable fixture failed: %v: %s", err, output)
	}
	home := filepath.Join(fleet, "node0")
	path := filepath.Join(home, "config", "genesis.json")
	original, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	// Pad app_state so the engine genesis has the given size.
	sized := func(size int) {
		var genesis map[string]json.RawMessage
		if err := json.Unmarshal(original, &genesis); err != nil {
			t.Fatal(err)
		}
		genesis["app_state"] = json.RawMessage(`{"chain_id":"dytallix-genesis-size-1","padding":""}`)
		raw, err := json.Marshal(genesis)
		if err != nil {
			t.Fatal(err)
		}
		padding := strings.Repeat("a", size-len(raw))
		genesis["app_state"] = json.RawMessage(`{"chain_id":"dytallix-genesis-size-1","padding":"` + padding + `"}`)
		if raw, err = json.Marshal(genesis); err != nil || len(raw) != size {
			t.Fatalf("padded genesis: %d bytes, %v", len(raw), err)
		}
		if err := os.WriteFile(path, raw, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, size := range []int{(8 << 20) + 4096, MaxGenesisBytes} {
		sized(size)
		if _, err := Load(home, Profile); err != nil {
			t.Fatalf("a %d-byte genesis was refused: %v", size, err)
		}
	}
	sized(MaxGenesisBytes + 1)
	if _, err := Load(home, Profile); err == nil {
		t.Fatal("a genesis beyond the engine bound was loaded")
	}
}
