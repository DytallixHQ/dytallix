package main

import (
	"bytes"
	"context"
	"crypto/sha3"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	abci "github.com/cometbft/cometbft/abci/types"
)

// writeSnapshot lays out a snapshot as the application writes it.
func writeSnapshot(t *testing.T, dir string, height uint64, chunks ...[]byte) []byte {
	t.Helper()
	target := filepath.Join(dir, snapshotName(height))
	if err := os.MkdirAll(target, 0o700); err != nil {
		t.Fatal(err)
	}
	hashes := ""
	for i, chunk := range chunks {
		if err := os.WriteFile(filepath.Join(target, fmt.Sprintf("chunk-%06d", i)), chunk, 0o600); err != nil {
			t.Fatal(err)
		}
		sum := sha3.Sum256(chunk)
		if i > 0 {
			hashes += ","
		}
		hashes += `"` + hex.EncodeToString(sum[:]) + `"`
	}
	metadata := []byte(fmt.Sprintf(`{"format":1,"chain_id":"c","height":%d,"app_hash":"%064d","state_digest":"%064d","retained_from":1,"entries":2,"bytes":6,"chunks":[%s]}`, height, 0, 0, hashes))
	if err := os.WriteFile(filepath.Join(target, metadataFile), metadata, 0o600); err != nil {
		t.Fatal(err)
	}
	return metadata
}

// Snapshots are served from files alone: the application child is absent.
func TestSnapshotsAreListedAndServedFromFiles(t *testing.T) {
	dir := t.TempDir()
	older := writeSnapshot(t, dir, 8, []byte("abc"), []byte("def"))
	newer := writeSnapshot(t, dir, 12, []byte("ghi"))
	// Not offered: an interrupted write, a directory without metadata, a
	// name that is not a height and metadata for another height.
	for _, name := range []string{".staging-00000000000000000016", "00000000000000000020", "0000000000000000002x"} {
		if err := os.MkdirAll(filepath.Join(dir, name), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	moved := writeSnapshot(t, dir, 24, []byte("jkl"))
	if err := os.WriteFile(filepath.Join(dir, snapshotName(24), metadataFile), bytes.Replace(moved, []byte(`"height":24`), []byte(`"height":25`), 1), 0o600); err != nil {
		t.Fatal(err)
	}
	a := application{snapshots: snapshotStore{dir: dir}}
	ctx := context.Background()
	listed, err := a.ListSnapshots(ctx, &abci.RequestListSnapshots{})
	if err != nil || len(listed.Snapshots) != 2 {
		t.Fatal(listed, err)
	}
	for i, want := range []struct {
		height   uint64
		chunks   uint32
		metadata []byte
	}{{12, 1, newer}, {8, 2, older}} {
		got := listed.Snapshots[i]
		hash := sha3.Sum256(want.metadata)
		if got.Height != want.height || got.Format != snapshotFormat || got.Chunks != want.chunks ||
			!bytes.Equal(got.Hash, hash[:]) || !bytes.Equal(got.Metadata, want.metadata) {
			t.Fatalf("snapshot %d: %v", i, got)
		}
	}
	for _, tc := range []struct {
		height uint64
		format uint32
		chunk  uint32
		want   string
	}{
		{8, 1, 1, "def"},
		{12, 1, 0, "ghi"},
		{8, 1, 2, ""},
		{8, 2, 0, ""},
		{16, 1, 0, ""},
		{24, 1, 0, ""},
	} {
		res, err := a.LoadSnapshotChunk(ctx, &abci.RequestLoadSnapshotChunk{Height: tc.height, Format: tc.format, Chunk: tc.chunk})
		if err != nil || string(res.Chunk) != tc.want {
			t.Fatalf("%+v: %q %v", tc, res.Chunk, err)
		}
	}
}

func TestSnapshotServingIsBounded(t *testing.T) {
	dir := t.TempDir()
	writeSnapshot(t, dir, 4, make([]byte, maxSnapshotChunk+1))
	a := application{snapshots: snapshotStore{dir: dir}}
	res, err := a.LoadSnapshotChunk(context.Background(), &abci.RequestLoadSnapshotChunk{Height: 4, Format: 1})
	if err != nil || res.Chunk != nil {
		t.Fatal("served an oversized chunk")
	}
	for _, store := range []snapshotStore{{}, {dir: filepath.Join(dir, "missing")}} {
		b := application{snapshots: store}
		listed, err := b.ListSnapshots(context.Background(), &abci.RequestListSnapshots{})
		if err != nil || len(listed.Snapshots) != 0 {
			t.Fatal(listed, err)
		}
	}
}
