//go:build dytallix_pqc_only && dytallix_pqc_ipc

package enginepqc

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestProductionCandidateFilePreflightAndStagingLoader(t *testing.T) {
	root, err := os.MkdirTemp("/tmp", "e01f-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"e01-candidate-file-check"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	fleet := filepath.Join(root, "fleet")
	cmd := exec.Command("go", "run", "-tags", "dytallix_pqc_only,dytallix_pqc_ipc", "./cmd/dytallix-comet-fixture", "--output", fleet, "--app-genesis", app, "--chain-id", "e01-candidate-file-check", "--genesis-time", "2026-09-25T00:00:00Z", "--base-port", "39650", "--p2p-profile", RemoteSeedProfile, "--peer-ips", "10.249.240.2,10.249.240.3,10.249.240.4,10.249.240.5")
	cmd.Dir = filepath.Join("..", "..")
	cmd.Env = append(os.Environ(), "GOPROXY=off", "GOSUMDB=off", "CGO_ENABLED=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("disposable private fixture failed: %v: %s", err, output)
	}
	home := filepath.Join(fleet, "node0")
	transport := filepath.Join(home, "config", "pqc_transport.json")
	raw, err := os.ReadFile(transport)
	if err != nil {
		t.Fatal(err)
	}
	var tc TransportConfig
	if err := json.Unmarshal(raw, &tc); err != nil {
		t.Fatal(err)
	}
	tc.Profile = ProductionCandidateProfile
	encoded, err := json.Marshal(tc)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(transport, encoded, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ValidateProductionCandidateFiles(home); err != nil {
		t.Fatalf("complete candidate file preflight failed: %v", err)
	}
	hashFile := func(path string) string {
		t.Helper()
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256(data)
		return hex.EncodeToString(sum[:])
	}
	binding := ProductionCandidateBinding{
		ChainID:         "e01-candidate-file-check",
		ConfigSHA256:    hashFile(filepath.Join(home, "config", "config.toml")),
		GenesisSHA256:   hashFile(filepath.Join(home, "config", "genesis.json")),
		TransportSHA256: hashFile(transport),
	}
	if err := ValidateProductionCandidateBinding(home, binding); err != nil {
		t.Fatalf("exact candidate binding rejected: %v", err)
	}
	changed := binding
	changed.TransportSHA256 = hashFile(app)
	if err := ValidateProductionCandidateBinding(home, changed); err == nil {
		t.Fatal("different transport digest accepted")
	}
	changed = binding
	changed.ChainID = "different-chain"
	if err := ValidateProductionCandidateBinding(home, changed); err == nil {
		t.Fatal("different approved chain accepted")
	}
	changed = binding
	changed.GenesisSHA256 = "ABCDEF"
	if err := ValidateProductionCandidateBinding(home, changed); err == nil {
		t.Fatal("noncanonical digest accepted")
	}
	statePath := filepath.Join(home, "data", "priv_validator_state.json")
	stateRaw, err := os.ReadFile(statePath)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(statePath, []byte("{"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ValidateProductionCandidateFiles(home); err == nil {
		t.Fatal("malformed validator signing state accepted")
	}
	if err := os.WriteFile(statePath, stateRaw, 0o600); err != nil {
		t.Fatal(err)
	}
	runtime, err := LoadCandidateForStaging(home)
	if err != nil || runtime.Transport.Profile != ProductionCandidateProfile {
		t.Fatalf("reserved staging candidate rejected: %v", err)
	}
	if _, err := Load(home, ProductionCandidateProfile); err == nil {
		t.Fatal("ordinary loader accepted candidate profile")
	}
}
