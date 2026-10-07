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

func TestPublicFileAcceptsPublishedAndRejectsWritableOrLinked(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "binding.json")
	if err := os.WriteFile(path, []byte("published"), 0o444); err != nil {
		t.Fatal(err)
	}
	// The installer's mode; privateFile refuses it.
	if got, err := publicFile(path, 16); err != nil || !bytes.Equal(got, []byte("published")) {
		t.Fatalf("published file: %q, %v", got, err)
	}
	if _, err := privateFile(path, 16); err == nil {
		t.Fatal("privateFile accepted a 0444 file")
	}
	if _, err := publicFile(path, 8); err == nil {
		t.Fatal("oversize published file accepted")
	}
	for _, mode := range []os.FileMode{0o664, 0o646, os.ModeSetuid | 0o444} {
		if err := os.Chmod(path, mode); err != nil {
			t.Fatal(err)
		}
		if _, err := publicFile(path, 16); err == nil {
			t.Fatalf("mode %o accepted", mode)
		}
	}
	if err := os.Chmod(path, 0o444); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(dir, "link.json")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := publicFile(link, 16); err == nil {
		t.Fatal("symlink accepted")
	}
	if err := os.Link(path, filepath.Join(dir, "hardlink.json")); err != nil {
		t.Fatal(err)
	}
	if _, err := publicFile(path, 16); err == nil {
		t.Fatal("multiply linked published file accepted")
	}
	if _, err := publicFile(dir, 16); err == nil {
		t.Fatal("directory accepted")
	}
}
