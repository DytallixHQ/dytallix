package enginepqc

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestPrivateFileRejectsUnsafeFileTypesAndModes(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	if err := os.WriteFile(path, []byte("private"), 0o600); err != nil {
		t.Fatal(err)
	}
	if got, err := privateFile(path, 16); err != nil || !bytes.Equal(got, []byte("private")) {
		t.Fatalf("owner-only file: %q, %v", got, err)
	}
	if _, err := privateFile(path, 6); err == nil {
		t.Fatal("oversize private file accepted")
	}
	if err := os.Chmod(path, 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := privateFile(path, 16); err == nil {
		t.Fatal("public private file accepted")
	}
	if err := os.Chmod(path, 0o600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(dir, "link.json")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := privateFile(link, 16); err == nil {
		t.Fatal("symlink accepted")
	}
	hardlink := filepath.Join(dir, "hardlink.json")
	if err := os.Link(path, hardlink); err != nil {
		t.Fatal(err)
	}
	if _, err := privateFile(path, 16); err == nil {
		t.Fatal("multiply linked private file accepted")
	}
	if _, err := privateFile(dir, 16); err == nil {
		t.Fatal("directory accepted")
	}
}
