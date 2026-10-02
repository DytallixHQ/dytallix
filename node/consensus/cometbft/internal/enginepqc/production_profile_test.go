//go:build production

package enginepqc

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/cometbft/cometbft/privval"
)

// productionFleet writes a disposable private-seed fleet with the
// development fixture command, then moves every node to the production
// transport profile: the same explicit endpoints, seed identities and pins.
func productionFleet(t *testing.T) string {
	t.Helper()
	root, err := os.MkdirTemp("/tmp", "a4p-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"dytallix-profile-check-1"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	fleet := filepath.Join(root, "fleet")
	cmd := exec.Command("go", "run", "./cmd/dytallix-comet-fixture", "--output", fleet, "--app-genesis", app,
		"--chain-id", "dytallix-profile-check-1", "--genesis-time", "2026-10-01T00:00:00Z",
		"--base-port", "39750", "--p2p-profile", RemoteSeedProfile,
		"--peer-ips", "10.249.241.2,10.249.241.3,10.249.241.4,10.249.241.5")
	cmd.Dir = filepath.Join("..", "..")
	cmd.Env = append(os.Environ(), "GOPROXY=off", "GOSUMDB=off", "CGO_ENABLED=0")
	if output, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("disposable private fixture failed: %v: %s", err, output)
	}
	for i := 0; i < 4; i++ {
		// The production profile needs an explicit redial cap (P01, 2 October
		// 2026: 60 s).
		setRedialCap(t, filepath.Join(fleet, "node"+string(rune('0'+i))), "1m0s")
		transport := filepath.Join(fleet, "node"+string(rune('0'+i)), "config", "pqc_transport.json")
		raw, err := os.ReadFile(transport)
		if err != nil {
			t.Fatal(err)
		}
		var tc TransportConfig
		if err := json.Unmarshal(raw, &tc); err != nil {
			t.Fatal(err)
		}
		tc.Profile = ProductionProfile
		encoded, err := json.Marshal(tc)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(transport, encoded, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return fleet
}

// setRedialCap rewrites a fixture home's persistent_peers_max_dial_period.
func setRedialCap(t *testing.T, home, value string) {
	t.Helper()
	path := filepath.Join(home, "config", "config.toml")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(string(raw), "\n")
	found := 0
	for i, line := range lines {
		if strings.HasPrefix(line, "persistent_peers_max_dial_period = ") {
			lines[i] = `persistent_peers_max_dial_period = "` + value + `"`
			found++
		}
	}
	if found != 1 {
		t.Fatalf("config.toml has %d redial cap lines", found)
	}
	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestProductionProfileRequiresARedialCap(t *testing.T) {
	fleet := productionFleet(t)
	home := filepath.Join(fleet, "node0")
	for _, value := range []string{"0s", "999ms", "1h0m1s"} {
		setRedialCap(t, home, value)
		if _, err := Load(home, ProductionProfile); err == nil || !strings.Contains(err.Error(), "persistent_peers_max_dial_period") {
			t.Fatalf("redial cap %s accepted: %v", value, err)
		}
	}
	for _, value := range []string{"1s", "1m0s", "1h0m0s"} {
		setRedialCap(t, home, value)
		if _, err := Load(home, ProductionProfile); err != nil {
			t.Fatalf("redial cap %s refused: %v", value, err)
		}
	}
}

func bindingOf(runtime *Runtime, role string) ProductionBinding {
	return ProductionBinding{
		Schema:                   1,
		Role:                     role,
		ChainID:                  runtime.Genesis.ChainID,
		ConfigSHA256:             hex.EncodeToString(runtime.configSHA256[:]),
		GenesisSHA256:            hex.EncodeToString(runtime.genesisSHA256[:]),
		TransportSHA256:          hex.EncodeToString(runtime.transportSHA256[:]),
		PeerPublicKeySHA256:      digestHex(runtime.NodeKey.PubKey().Bytes()),
		ValidatorPublicKeySHA256: digestHex(runtime.Validator.Key.PubKey.Bytes()),
	}
}

func TestProductionProfileLoadsOnlyWithItsBinding(t *testing.T) {
	fleet := productionFleet(t)
	home := filepath.Join(fleet, "node0")
	runtime, err := Load(home, ProductionProfile)
	if err != nil {
		t.Fatalf("production profile refused: %v", err)
	}
	binding := bindingOf(runtime, RoleValidator)
	if err := ValidateProductionBinding(runtime, binding); err != nil {
		t.Fatalf("exact binding refused: %v", err)
	}
	// A genesis validator cannot run as a sentry or endpoint.
	for _, role := range []string{RoleSentry, RoleEndpoint} {
		other := binding
		other.Role = role
		if err := ValidateProductionBinding(runtime, other); err == nil {
			t.Fatalf("a genesis validator ran as %s", role)
		}
	}
	for name, change := range map[string]func(*ProductionBinding){
		"chain":         func(b *ProductionBinding) { b.ChainID = "another-chain" },
		"config":        func(b *ProductionBinding) { b.ConfigSHA256 = b.GenesisSHA256 },
		"transport":     func(b *ProductionBinding) { b.TransportSHA256 = b.GenesisSHA256 },
		"peer key":      func(b *ProductionBinding) { b.PeerPublicKeySHA256 = b.GenesisSHA256 },
		"validator key": func(b *ProductionBinding) { b.ValidatorPublicKeySHA256 = b.GenesisSHA256 },
		"uppercase":     func(b *ProductionBinding) { b.GenesisSHA256 = strings.ToUpper(b.GenesisSHA256) },
		"role":          func(b *ProductionBinding) { b.Role = "observer" },
		"schema":        func(b *ProductionBinding) { b.Schema = 2 },
	} {
		changed := binding
		change(&changed)
		if err := ValidateProductionBinding(runtime, changed); err == nil {
			t.Fatalf("binding with another %s accepted", name)
		}
	}
	// The binding file round-trips canonically.
	raw, _ := json.Marshal(binding)
	path := filepath.Join(home, "config", "binding.json")
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	loaded, err := LoadProductionBinding(path)
	if err != nil || loaded != binding {
		t.Fatalf("binding file: %v", err)
	}
	// A production build loads nothing else.
	for _, profile := range []string{Profile, SeedProfile, RemoteSeedProfile} {
		if _, err := Load(home, profile); err == nil {
			t.Fatalf("%s loaded in a production build", profile)
		}
	}
	if _, err := LoadCandidateForStaging(home); err == nil {
		t.Fatal("candidate staging loaded in a production build")
	}
}

func TestProductionProfileBindingRecord(t *testing.T) {
	fleet := productionFleet(t)
	runtime, err := Load(filepath.Join(fleet, "node3"), ProductionProfile)
	if err != nil {
		t.Fatal(err)
	}
	binding, err := NewProductionBinding(runtime, RoleValidator)
	if err != nil || binding != bindingOf(runtime, RoleValidator) {
		t.Fatalf("binding record: %v", err)
	}
	for _, role := range []string{RoleSentry, RoleEndpoint, "observer"} {
		if _, err := NewProductionBinding(runtime, role); err == nil {
			t.Fatalf("a genesis validator bound as %s", role)
		}
	}
	public, err := PeerSeedPublicKey(filepath.Join(fleet, "node3"))
	if err != nil || !bytes.Equal(public, runtime.NodeKey.PubKey().Bytes()) {
		t.Fatalf("seed public key differs from the loaded peer key: %v", err)
	}
}

func TestProductionProfileSentryKeyIsOutsideGenesis(t *testing.T) {
	fleet := productionFleet(t)
	home := filepath.Join(fleet, "node1")
	// A sentry carries a validator key the genesis set lacks.
	privval.GenFilePV(filepath.Join(home, "config", "priv_validator_key.json"),
		filepath.Join(home, "data", "priv_validator_state.json")).Save()
	runtime, err := Load(home, ProductionProfile)
	if err != nil {
		t.Fatalf("sentry home refused: %v", err)
	}
	if err := ValidateProductionBinding(runtime, bindingOf(runtime, RoleSentry)); err != nil {
		t.Fatalf("sentry binding refused: %v", err)
	}
	if err := ValidateProductionBinding(runtime, bindingOf(runtime, RoleValidator)); err == nil {
		t.Fatal("a key outside genesis ran as a validator")
	}
}

func TestProductionProfileRequiresSeedIdentity(t *testing.T) {
	fleet := productionFleet(t)
	home := filepath.Join(fleet, "node2")
	if err := os.WriteFile(filepath.Join(home, "config", "node_key.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := Load(home, ProductionProfile); err == nil || !strings.Contains(err.Error(), "packed peer key") {
		t.Fatalf("packed peer key accepted: %v", err)
	}
}
