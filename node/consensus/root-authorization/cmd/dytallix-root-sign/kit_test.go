package main

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// A rehearsal of the key ceremony with throwaway kits: five kits, paper
// checks, a drive restored from paper, proofs of possession and the genesis
// signer policy from the five kits' genesis keys.
func TestKitCeremony(t *testing.T) {
	dir := t.TempDir()
	public := filepath.Join(dir, "public")
	if err := os.Mkdir(public, 0o755); err != nil {
		t.Fatal(err)
	}
	drive := func(n int) string { return filepath.Join(dir, fmt.Sprintf("kit-%d-drive", n)) }
	for n := 1; n <= 5; n++ {
		if err := os.Mkdir(drive(n), 0o700); err != nil {
			t.Fatal(err)
		}
		output := runOK(t, "kit", "-number", fmt.Sprint(n), "-private-out", drive(n), "-public-out", public)
		if !strings.Contains(output, fmt.Sprintf("dytallix-kit-%d ", n)) {
			t.Fatalf("kit %d did not print its paper line", n)
		}
		for _, purpose := range []string{"genesis", "upgrade", "freeze", "resume"} {
			info, err := os.Stat(filepath.Join(drive(n), kitName(n, purpose, ".key")))
			if err != nil || info.Mode().Perm() != 0o600 || info.Size() != 128 {
				t.Fatal("private key must be 128 bytes, mode 0600", err)
			}
		}
	}
	// Outputs are never overwritten.
	if err := run([]string{"kit", "-number", "1", "-private-out", drive(1), "-public-out", public}, &bytes.Buffer{}); err == nil {
		t.Fatal("a kit overwrote an existing kit")
	}
	if err := run([]string{"kit", "-number", "6", "-private-out", drive(1), "-public-out", public}, &bytes.Buffer{}); err == nil {
		t.Fatal("made kit 6")
	}

	// The paper line checks against the public keys; a typo does not.
	paper := filepath.Join(drive(3), "kit-3-paper.txt")
	if out := runOK(t, "kit-check", "-paper", paper, "-public", public); !strings.Contains(out, "kit 3 paper line matches") {
		t.Fatal(out)
	}
	line, _ := os.ReadFile(paper)
	typo := filepath.Join(dir, "typo.txt")
	bad := append([]byte{}, line...)
	first := len("dytallix-kit-3 ")
	if bad[first] == 'f' {
		bad[first] = '0'
	} else {
		bad[first] = 'f'
	}
	if err := os.WriteFile(typo, bad, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"kit-check", "-paper", typo, "-public", public}, &bytes.Buffer{}); err == nil {
		t.Fatal("a mistyped paper line checked")
	}

	// A lost drive comes back from paper, byte for byte.
	restored := filepath.Join(dir, "restored")
	if err := os.Mkdir(restored, 0o700); err != nil {
		t.Fatal(err)
	}
	runOK(t, "kit-restore", "-paper", paper, "-public", public, "-private-out", restored)
	for _, purpose := range []string{"genesis", "upgrade", "freeze", "resume"} {
		original, _ := os.ReadFile(filepath.Join(drive(3), kitName(3, purpose, ".key")))
		again, _ := os.ReadFile(filepath.Join(restored, kitName(3, purpose, ".key")))
		if !bytes.Equal(original, again) {
			t.Fatalf("restored %s key differs", purpose)
		}
	}

	// Proofs of possession, with and without an epoch.
	for _, c := range []struct{ purpose, epoch string }{{"genesis", "none"}, {"freeze", "1"}} {
		proof := filepath.Join(public, fmt.Sprintf("kit-2-%s-proof.json", c.purpose))
		runOK(t, "prove", "-private-key", filepath.Join(drive(2), kitName(2, c.purpose, ".key")),
			"-public-key", filepath.Join(public, kitName(2, c.purpose, ".json")), "-chain-id", "dytallix-mainnet-1",
			"-controller", "founder", "-purpose", c.purpose, "-epoch", c.epoch, "-session", "rehearsal-1", "-out", proof)
		if out := runOK(t, "verify-proof", "-proof", proof); !strings.Contains(out, "verified "+c.purpose) {
			t.Fatal(out)
		}
	}
	// The private key must match the public record it claims.
	if err := run([]string{"prove", "-private-key", filepath.Join(drive(2), kitName(2, "upgrade", ".key")),
		"-public-key", filepath.Join(public, kitName(2, "resume", ".json")), "-chain-id", "dytallix-mainnet-1",
		"-controller", "founder", "-purpose", "resume", "-epoch", "1", "-session", "rehearsal-1",
		"-out", filepath.Join(dir, "wrong.json")}, &bytes.Buffer{}); err == nil {
		t.Fatal("proved with a key that is not the record's")
	}

	// The five kits' genesis keys make the genesis signer policy.
	var genesis []string
	for n := 1; n <= 5; n++ {
		genesis = append(genesis, filepath.Join(public, kitName(n, "genesis", ".json")))
	}
	runOK(t, append([]string{"policy", "-chain-id", "dytallix-mainnet-1", "-out", filepath.Join(public, "policy.json")}, genesis...)...)
}
