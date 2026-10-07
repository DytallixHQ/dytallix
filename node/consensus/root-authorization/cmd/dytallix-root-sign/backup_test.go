package main

import (
	"bytes"
	"crypto/rand"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// makeSnapshot writes a directory shaped like a snapshot: chunk files and a
// metadata file, one larger than a backup chunk.
func makeSnapshot(t *testing.T, dir string) map[string][]byte {
	t.Helper()
	files := map[string][]byte{
		"17280/metadata.json":   []byte(`{"height":17280}`),
		"17280/chunks/00000000": make([]byte, 1<<20+4321),
		"17280/chunks/00000001": []byte("last chunk"),
	}
	_, _ = rand.Read(files["17280/chunks/00000000"])
	for path, data := range files {
		full := filepath.Join(dir, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(full), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return files
}

func TestBackupSealCheckAndOpen(t *testing.T) {
	dir := t.TempDir()
	codeFile := filepath.Join(dir, "backup-code")
	output := runOK(t, "backup-code", "-chain", "dytallix-staging-1", "-out", codeFile)
	var line string
	for _, row := range strings.Split(output, "\n") {
		if strings.HasPrefix(row, "dytallix-backup-dytallix-staging-1 ") {
			line = row
		}
	}
	if len(strings.Fields(line)) != 18 {
		t.Fatalf("no paper line in %q", output)
	}
	if info, err := os.Stat(codeFile); err != nil || info.Mode().Perm() != 0o600 {
		t.Fatal("the code file must be owner-only", err)
	}
	snapshots := filepath.Join(dir, "snapshots")
	files := makeSnapshot(t, snapshots)
	copyFile := filepath.Join(dir, "copy.bin")
	sealed := runOK(t, "backup-seal", "-code-file", codeFile, "-height", "17280", "-in", snapshots, "-out", copyFile)
	if !strings.Contains(sealed, "chain dytallix-staging-1, height 17280, 5 entries") {
		t.Fatalf("seal: %q", sealed)
	}
	if _, err := os.Stat(copyFile + ".partial"); err == nil {
		t.Fatal("the partial file was left behind")
	}
	raw, _ := os.ReadFile(copyFile)
	if bytes.Contains(raw, []byte("last chunk")) || bytes.Contains(raw, []byte(`"height":17280}`)) {
		t.Fatal("the copy holds plaintext")
	}
	paper := filepath.Join(dir, "paper.txt")
	if err := os.WriteFile(paper, []byte(line+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if out := runOK(t, "backup-check", "-paper", paper, "-in", copyFile); !strings.Contains(out, "5 entries") {
		t.Fatalf("check: %q", out)
	}
	restored := filepath.Join(dir, "restored")
	runOK(t, "backup-open", "-paper", paper, "-in", copyFile, "-out", restored)
	for path, want := range files {
		got, err := os.ReadFile(filepath.Join(restored, filepath.FromSlash(path)))
		info, _ := os.Stat(filepath.Join(restored, filepath.FromSlash(path)))
		if err != nil || !bytes.Equal(got, want) || info.Mode().Perm() != 0o600 {
			t.Fatalf("%s did not restore exactly and owner-only: %v", path, err)
		}
	}
	// Never overwrites: the copy, the restore directory.
	if err := run([]string{"backup-seal", "-code-file", codeFile, "-height", "17280", "-in", snapshots, "-out", copyFile}, &bytes.Buffer{}); err == nil {
		t.Fatal("a copy was overwritten")
	}
	if err := run([]string{"backup-open", "-paper", paper, "-in", copyFile, "-out", restored}, &bytes.Buffer{}); err == nil {
		t.Fatal("an existing directory was written into")
	}
	// A damaged copy is refused before anything is written.
	raw[len(raw)-5] ^= 1
	damaged := filepath.Join(dir, "damaged.bin")
	if err := os.WriteFile(damaged, raw, 0o644); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(dir, "from-damaged")
	if err := run([]string{"backup-open", "-paper", paper, "-in", damaged, "-out", target}, &bytes.Buffer{}); err == nil {
		t.Fatal("a damaged copy opened")
	}
	if _, err := os.Stat(target); err == nil {
		t.Fatal("a damaged copy wrote its directory")
	}
	// Another chain's code does not open it.
	other := filepath.Join(dir, "other-code")
	otherOut := runOK(t, "backup-code", "-chain", "dytallix-staging-2", "-out", other)
	otherLine := otherOut[strings.Index(otherOut, "dytallix-backup-"):]
	otherPaper := filepath.Join(dir, "other-paper.txt")
	if err := os.WriteFile(otherPaper, []byte(otherLine), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"backup-check", "-paper", otherPaper, "-in", copyFile}, &bytes.Buffer{}); err == nil {
		t.Fatal("another chain's code opened the copy")
	}
}

func TestBackupSealRefusesUnsafeInputs(t *testing.T) {
	dir := t.TempDir()
	codeFile := filepath.Join(dir, "backup-code")
	runOK(t, "backup-code", "-chain", "dytallix-staging-1", "-out", codeFile)
	snapshots := filepath.Join(dir, "snapshots")
	makeSnapshot(t, snapshots)
	if err := os.Symlink("/etc/hosts", filepath.Join(snapshots, "link")); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"backup-seal", "-code-file", codeFile, "-height", "17280", "-in", snapshots, "-out", filepath.Join(dir, "a.bin")}, &bytes.Buffer{}); err == nil {
		t.Fatal("sealed a symbolic link")
	}
	if _, err := os.Stat(filepath.Join(dir, "a.bin")); err == nil {
		t.Fatal("a failed seal left a copy")
	}
	if err := os.Remove(filepath.Join(snapshots, "link")); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(codeFile, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"backup-seal", "-code-file", codeFile, "-height", "17280", "-in", snapshots, "-out", filepath.Join(dir, "b.bin")}, &bytes.Buffer{}); err == nil {
		t.Fatal("read a code file others can read")
	}
	if err := os.Chmod(codeFile, 0o600); err != nil {
		t.Fatal(err)
	}
	for _, height := range []string{"0", "-1", "x"} {
		if err := run([]string{"backup-seal", "-code-file", codeFile, "-height", height, "-in", snapshots, "-out", filepath.Join(dir, "c.bin")}, &bytes.Buffer{}); err == nil {
			t.Fatalf("height %s accepted", height)
		}
	}
}

// A typed code is read once and serves both the check and the extraction.
func TestBackupOpenWithATypedCode(t *testing.T) {
	dir := t.TempDir()
	codeFile := filepath.Join(dir, "backup-code")
	output := runOK(t, "backup-code", "-chain", "dytallix-staging-1", "-out", codeFile)
	line := output[strings.Index(output, "dytallix-backup-"):]
	snapshots := filepath.Join(dir, "snapshots")
	makeSnapshot(t, snapshots)
	copyFile := filepath.Join(dir, "copy.bin")
	runOK(t, "backup-seal", "-code-file", codeFile, "-height", "17280", "-in", snapshots, "-out", copyFile)
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	stdin := os.Stdin
	os.Stdin = reader
	defer func() { os.Stdin = stdin }()
	if _, err := writer.WriteString(strings.TrimSpace(line) + "\n"); err != nil {
		t.Fatal(err)
	}
	writer.Close()
	restored := filepath.Join(dir, "restored")
	runOK(t, "backup-open", "-paper", "-", "-in", copyFile, "-out", restored)
	if _, err := os.Stat(filepath.Join(restored, "17280", "metadata.json")); err != nil {
		t.Fatal("the typed code did not restore the snapshot", err)
	}
}
