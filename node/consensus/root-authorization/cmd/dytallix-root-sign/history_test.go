package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHistorySealCheckAndOpen(t *testing.T) {
	dir := t.TempDir()
	codeFile := filepath.Join(dir, "history-code")
	output := runOK(t, "history-code", "-chain", "dytallix-staging-1", "-out", codeFile)
	var line string
	for _, row := range strings.Split(output, "\n") {
		if strings.HasPrefix(row, "dytallix-history-dytallix-staging-1 ") {
			line = row
		}
	}
	if line == "" {
		t.Fatalf("no paper line in %q", output)
	}
	if info, err := os.Stat(codeFile); err != nil || info.Mode().Perm() != 0o600 {
		t.Fatalf("code file: %v", err)
	}
	day := bytes.Repeat([]byte(`{"t":1,"h":2}`+"\n"), 100_000)
	input := filepath.Join(dir, "2026-10-10.jsonl.gz")
	if err := os.WriteFile(input, day, 0o600); err != nil {
		t.Fatal(err)
	}
	copyFile := filepath.Join(dir, "copy.bin")
	output = runOK(t, "history-seal", "-code-file", codeFile, "-host", "validator-1", "-day", "2026-10-10", "-in", input,
		"-out", copyFile)
	if !strings.Contains(output, "host validator-1, day 2026-10-10") {
		t.Fatalf("seal output %q", output)
	}
	if _, err := os.Stat(copyFile + ".partial"); !os.IsNotExist(err) {
		t.Fatal("the partial copy was left behind")
	}
	paper := filepath.Join(dir, "paper.txt")
	if err := os.WriteFile(paper, []byte(line+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if out := runOK(t, "history-check", "-paper", paper, "-in", copyFile); !strings.Contains(out, "1400000 bytes") {
		t.Fatalf("check output %q", out)
	}
	opened := filepath.Join(dir, "opened.jsonl.gz")
	runOK(t, "history-open", "-paper", paper, "-in", copyFile, "-out", opened)
	if raw, err := os.ReadFile(opened); err != nil || !bytes.Equal(raw, day) {
		t.Fatalf("opened: %v", err)
	}
	if info, _ := os.Stat(opened); info.Mode().Perm() != 0o600 {
		t.Fatal("the opened history must be owner-only")
	}
	// Never overwritten.
	if err := run([]string{"history-open", "-paper", paper, "-in", copyFile, "-out", opened}, &bytes.Buffer{}); err == nil {
		t.Fatal("history-open overwrote its output")
	}
	if err := run([]string{"history-seal", "-code-file", codeFile, "-host", "validator-1", "-day", "2026-10-10",
		"-in", input, "-out", copyFile}, &bytes.Buffer{}); err == nil {
		t.Fatal("history-seal overwrote its output")
	}
}

func TestHistoryRefusals(t *testing.T) {
	dir := t.TempDir()
	codeFile := filepath.Join(dir, "history-code")
	runOK(t, "history-code", "-chain", "dytallix-staging-1", "-out", codeFile)
	input := filepath.Join(dir, "day")
	if err := os.WriteFile(input, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	seal := func(host, day, out string) error {
		return run([]string{"history-seal", "-code-file", codeFile, "-host", host, "-day", day, "-in", input,
			"-out", filepath.Join(dir, out)}, &bytes.Buffer{})
	}
	for _, c := range [][3]string{{"../x", "2026-10-10", "a"}, {"validator-1", "2026-02-30", "b"}} {
		if err := seal(c[0], c[1], c[2]); err == nil {
			t.Fatalf("sealed host %q day %q", c[0], c[1])
		}
		if _, err := os.Stat(filepath.Join(dir, c[2])); !os.IsNotExist(err) {
			t.Fatal("a refused seal left a file")
		}
	}
	// A code file others can read is refused.
	if err := os.Chmod(codeFile, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := seal("validator-1", "2026-10-10", "c"); err == nil || !strings.Contains(err.Error(), "only by its owner") {
		t.Fatalf("an open code file was accepted: %v", err)
	}
	if err := os.Chmod(codeFile, 0o600); err != nil {
		t.Fatal(err)
	}
	// The backup code does not open history, and another chain's history
	// code is named as such.
	if err := seal("validator-1", "2026-10-10", "copy"); err != nil {
		t.Fatal(err)
	}
	backupLine := strings.TrimSpace(strings.Split(runOK(t, "backup-code", "-chain", "dytallix-staging-1", "-out",
		filepath.Join(dir, "backup-code")), "\n")[1])
	other := strings.TrimSpace(strings.Split(runOK(t, "history-code", "-chain", "dytallix-other-1", "-out",
		filepath.Join(dir, "other-code")), "\n")[1])
	for _, c := range []struct{ line, message string }{{backupLine, ""}, {other, "the history code is for dytallix-other-1"}} {
		paper := filepath.Join(dir, "paper")
		os.Remove(paper)
		if err := os.WriteFile(paper, []byte(c.line+"\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		err := run([]string{"history-open", "-paper", paper, "-in", filepath.Join(dir, "copy"), "-out",
			filepath.Join(dir, "opened")}, &bytes.Buffer{})
		if err == nil || !strings.Contains(err.Error(), c.message) {
			t.Fatalf("opened with %q: %v", c.line, err)
		}
		if _, err := os.Stat(filepath.Join(dir, "opened")); !os.IsNotExist(err) {
			t.Fatal("a refused open left a file")
		}
	}
}
