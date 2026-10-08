package main

import (
	"bytes"
	"crypto/sha3"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func writeCopy(t *testing.T, metadata string, chunks ...[]byte) string {
	t.Helper()
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, restoreMetadata), []byte(metadata), 0o600); err != nil {
		t.Fatal(err)
	}
	for i, chunk := range chunks {
		if err := os.WriteFile(filepath.Join(dir, "chunk-00000"+string(rune('0'+i))), chunk, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return dir
}

func TestReadCopyDescribesTheSnapshotTheBridgeWouldOffer(t *testing.T) {
	metadata := `{"format":1,"chain_id":"dytallix-staging-1","height":20,"app_hash":"ab","state_digest":"cd","retained_from":1,"entries":2,"bytes":6,"chunks":["x","y"]}`
	dir := writeCopy(t, metadata, []byte("abc"), []byte("def"))
	local, err := readCopy(dir, "dytallix-staging-1")
	if err != nil {
		t.Fatal(err)
	}
	hash := sha3.Sum256([]byte(metadata))
	if local.Height != 20 || local.Format != 1 || local.Chunks != 2 || !bytes.Equal(local.Hash, hash[:]) || string(local.Metadata) != metadata {
		t.Fatalf("%+v", local)
	}
	if chunk, err := local.Chunk(1); err != nil || string(chunk) != "def" {
		t.Fatal(string(chunk), err)
	}
	if _, err := local.Chunk(2); err == nil {
		t.Fatal("read a chunk the copy does not have")
	}
	// A chunk over the application's bound is refused.
	if err := os.WriteFile(filepath.Join(dir, "chunk-000000"), make([]byte, maxRestoreChunk+1), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := local.Chunk(0); err == nil || !strings.Contains(err.Error(), "bound") {
		t.Fatal("read an oversized chunk", err)
	}
}

func TestReadCopyRefusesAnotherChainOrAnEmptySnapshot(t *testing.T) {
	good := `{"format":1,"chain_id":"dytallix-staging-1","height":20,"chunks":["x"]}`
	for name, dir := range map[string]string{
		"another chain": writeCopy(t, `{"format":1,"chain_id":"other","height":20,"chunks":["x"]}`),
		"no height":     writeCopy(t, `{"format":1,"chain_id":"dytallix-staging-1","height":0,"chunks":["x"]}`),
		"no chunks":     writeCopy(t, `{"format":1,"chain_id":"dytallix-staging-1","height":20,"chunks":[]}`),
		"not json":      writeCopy(t, `metadata`),
		"no metadata":   t.TempDir(),
		"relative":      "copy",
		"unclean":       writeCopy(t, good) + "/.",
	} {
		if _, err := readCopy(dir, "dytallix-staging-1"); err == nil {
			t.Fatal("accepted", name)
		}
	}
}
