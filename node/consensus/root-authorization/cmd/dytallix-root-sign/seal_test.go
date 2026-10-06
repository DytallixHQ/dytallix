package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// makeHome writes a staging node home with the validator's secret files.
func makeHome(t *testing.T, dir string) (string, map[string][]byte) {
	t.Helper()
	home := filepath.Join(dir, "staging-home")
	files := map[string][]byte{
		"config/pqc_peer_seed.bin":       bytes.Repeat([]byte{7}, 32),
		"config/priv_validator_key.json": []byte(`{"synthetic":"validator key"}`),
		"data/priv_validator_state.json": []byte(`{"height":"0","round":0,"step":0}`),
	}
	for _, sub := range []string{"config", "data"} {
		if err := os.MkdirAll(filepath.Join(home, sub), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	for path, data := range files {
		if err := os.WriteFile(filepath.Join(home, path), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return home, files
}

func paperLine(t *testing.T, output string) string {
	t.Helper()
	for _, line := range strings.Split(output, "\n") {
		if strings.HasPrefix(line, "dytallix-seal-") {
			return line
		}
	}
	t.Fatalf("no paper line in %q", output)
	return ""
}

func TestSealCheckAndUnseal(t *testing.T) {
	dir := t.TempDir()
	home, files := makeHome(t, dir)
	sealed := filepath.Join(dir, "validator-1.sealed.json")
	paths := []string{"config/pqc_peer_seed.bin", "config/priv_validator_key.json", "data/priv_validator_state.json"}
	output := runOK(t, append([]string{"seal", "-label", "validator-1", "-home", home, "-out", sealed}, paths...)...)
	line := paperLine(t, output)
	if !strings.HasPrefix(line, "dytallix-seal-validator-1 ") || len(strings.Fields(line)) != 18 {
		t.Fatalf("unexpected paper line %q", line)
	}
	raw, err := os.ReadFile(sealed)
	if err != nil || bytes.Contains(raw, []byte(strings.Join(strings.Fields(line)[1:3], ""))) {
		t.Fatal("the sealed record must not hold its code", err)
	}
	paper := filepath.Join(dir, "paper.txt")
	if err := os.WriteFile(paper, []byte(line+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if out := runOK(t, "seal-check", "-paper", paper, "-sealed", sealed); !strings.Contains(out, "opens its sealed keys (3 files)") {
		t.Fatalf("seal-check: %q", out)
	}
	// A typed digit off fails the check group.
	typo := []byte(line)
	typo[len("dytallix-seal-validator-1 ")] ^= 1
	if err := os.WriteFile(paper+".typo", typo, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"seal-check", "-paper", paper + ".typo", "-sealed", sealed}, &bytes.Buffer{}); err == nil {
		t.Fatal("a mistyped code passed")
	}
	// Unseal into a fresh home with its directories made.
	target := filepath.Join(dir, "node")
	for _, sub := range []string{"", "config", "data"} {
		if err := os.Mkdir(filepath.Join(target, sub), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	if err := run([]string{"unseal", "-paper", paper, "-sealed", sealed, "-label", "sentry-1", "-out", target}, &bytes.Buffer{}); err == nil {
		t.Fatal("unsealed for another host")
	}
	runOK(t, "unseal", "-paper", paper, "-sealed", sealed, "-label", "validator-1", "-out", target)
	for path, want := range files {
		info, err := os.Stat(filepath.Join(target, path))
		got, readErr := os.ReadFile(filepath.Join(target, path))
		if err != nil || readErr != nil || info.Mode().Perm() != 0o600 || !bytes.Equal(got, want) {
			t.Fatalf("%s: not unsealed owner-only and exact (%v %v)", path, err, readErr)
		}
	}
	// Never overwrites.
	if err := run([]string{"unseal", "-paper", paper, "-sealed", sealed, "-label", "validator-1", "-out", target}, &bytes.Buffer{}); err == nil {
		t.Fatal("unseal replaced existing files")
	}
}

func TestSealRefusesUnsafeInputs(t *testing.T) {
	dir := t.TempDir()
	home, _ := makeHome(t, dir)
	if err := os.Chmod(filepath.Join(home, "config", "pqc_peer_seed.bin"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"seal", "-label", "validator-1", "-home", home, "-out", filepath.Join(dir, "a.json"), "config/pqc_peer_seed.bin"}, &bytes.Buffer{}); err == nil {
		t.Fatal("sealed a file others can read")
	}
	if err := os.WriteFile(filepath.Join(dir, "outside"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"seal", "-label", "validator-1", "-home", home, "-out", filepath.Join(dir, "b.json"), "../outside"}, &bytes.Buffer{}); err == nil {
		t.Fatal("sealed a path outside the home")
	}
	if err := run([]string{"seal", "-label", "validator-1", "-home", "relative", "-out", filepath.Join(dir, "c.json"), "config/priv_validator_key.json"}, &bytes.Buffer{}); err == nil {
		t.Fatal("accepted a relative home")
	}
	if err := run([]string{"seal", "-label", "validator-1", "-home", home, "-out", filepath.Join(dir, "d.json")}, &bytes.Buffer{}); err == nil {
		t.Fatal("sealed no files")
	}
}

// A typed paper line ends at Enter; the terminal stays open.
func TestTypedPaperLineEndsAtEnter(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer writer.Close()
	stdin := os.Stdin
	os.Stdin = reader
	defer func() { os.Stdin = stdin }()
	if _, err := writer.WriteString("dytallix-seal-validator-1 0000\n"); err != nil {
		t.Fatal(err)
	}
	done := make(chan []byte, 1)
	go func() {
		raw, _ := readPaper("-")
		done <- raw
	}()
	select {
	case raw := <-done:
		if string(raw) != "dytallix-seal-validator-1 0000\n" {
			t.Fatalf("read %q", raw)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("readPaper waited for the end of input")
	}
}
