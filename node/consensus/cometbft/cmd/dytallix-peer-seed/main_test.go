package main

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
)

func privateHome(t *testing.T) string {
	t.Helper()
	home := filepath.Join(t.TempDir(), "home")
	if err := os.MkdirAll(filepath.Join(home, "config"), 0o700); err != nil {
		t.Fatal(err)
	}
	return home
}

func TestGenerateWritesOneOwnerOnlySeedAndItsPublicKey(t *testing.T) {
	home := privateHome(t)
	record, err := run([]string{"generate", "--home", home})
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, "config", enginepqc.SeedFileName)
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0o600 || info.Size() != 32 {
		t.Fatalf("seed file: %v %v", info, err)
	}
	generated := record.(map[string]any)
	public, err := base64.StdEncoding.DecodeString(generated["public_key_base64"].(string))
	if err != nil || len(public) != 1952 || generated["version"] != outputVersion {
		t.Fatalf("public record: %v", generated)
	}
	// public re-derives the same record from the seed.
	again, err := run([]string{"public", "--home", home})
	if err != nil {
		t.Fatal(err)
	}
	for _, key := range []string{"public_key_base64", "public_key_sha256", "peer_id"} {
		if again.(map[string]any)[key] != generated[key] {
			t.Fatalf("%s differs", key)
		}
	}
	// A seed is never replaced.
	if _, err := run([]string{"generate", "--home", home}); err == nil {
		t.Fatal("an existing seed was replaced")
	}
}

func TestGenerateRefusesPackedKeysAndOpenHomes(t *testing.T) {
	home := privateHome(t)
	if err := os.WriteFile(filepath.Join(home, "config", "node_key.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := run([]string{"generate", "--home", home}); err == nil || !strings.Contains(err.Error(), "packed peer key") {
		t.Fatalf("packed key home accepted: %v", err)
	}
	open := privateHome(t)
	if err := os.Chmod(filepath.Join(open, "config"), 0o750); err != nil {
		t.Fatal(err)
	}
	if _, err := run([]string{"generate", "--home", open}); err == nil {
		t.Fatal("group-readable config directory accepted")
	}
	if _, err := run([]string{"generate", "--home", "relative/home"}); err == nil {
		t.Fatal("relative home accepted")
	}
	// A seed others can read is refused.
	seeded := privateHome(t)
	if _, err := run([]string{"generate", "--home", seeded}); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(filepath.Join(seeded, "config", enginepqc.SeedFileName), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := run([]string{"public", "--home", seeded}); err == nil {
		t.Fatal("readable seed accepted")
	}
}

func TestUsageAndBuildBoundary(t *testing.T) {
	home := privateHome(t)
	for _, args := range [][]string{
		nil,
		{"rotate", "--home", home},
		{"generate"},
		{"generate", "--home", home, "--role", "sentry"},
		{"binding", "--home", home},
		{"public", "--home", home, "extra"},
	} {
		if _, err := run(args); err == nil {
			t.Fatalf("%v accepted", args)
		}
	}
	// Only a production build loads the production transport profile.
	if !enginepqc.ProductionBuild {
		if _, err := run([]string{"binding", "--home", home, "--role", "sentry"}); err == nil ||
			!strings.Contains(err.Error(), "no production transport profile") {
			t.Fatalf("development binding: %v", err)
		}
	}
}
