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

func restoreChild(t *testing.T, request, result string) application {
	t.Helper()
	t.Setenv("DYTALLIX_BRIDGE_TEST_REQUEST", request)
	t.Setenv("DYTALLIX_BRIDGE_TEST_RESPONSE", `{"ok":true,"result":{"result":"`+result+`"}}`)
	return application{child: testChild(t, "restore")}
}

// The offer reaches the application with the verified application hash in
// hex; every result maps to its engine result, and anything else aborts.
func TestOfferSnapshotForwardsTheTrustedApplicationHash(t *testing.T) {
	req := &abci.RequestOfferSnapshot{
		Snapshot: &abci.Snapshot{Height: 12, Format: 1, Chunks: 2, Hash: []byte{1, 2}, Metadata: []byte("{}")},
		AppHash:  bytes.Repeat([]byte{0xab}, 32),
	}
	request := `offer_snapshot {"app_hash":"` + hex.EncodeToString(req.AppHash) + `","chunks":2,"format":1,"hash":"AQI=","height":12,"metadata":"e30="}`
	for result, want := range map[string]abci.ResponseOfferSnapshot_Result{
		"accept":        abci.ResponseOfferSnapshot_ACCEPT,
		"reject":        abci.ResponseOfferSnapshot_REJECT,
		"reject_format": abci.ResponseOfferSnapshot_REJECT_FORMAT,
		"abort":         abci.ResponseOfferSnapshot_ABORT,
		"other":         abci.ResponseOfferSnapshot_ABORT,
	} {
		a := restoreChild(t, request, result)
		res, err := a.OfferSnapshot(context.Background(), req)
		if err != nil || res.Result != want {
			t.Fatalf("%s: %v %v", result, res, err)
		}
	}
	// A changed request makes the application call fail, which aborts.
	a := restoreChild(t, "offer_snapshot {}", "accept")
	if res, err := a.OfferSnapshot(context.Background(), req); err != nil || res.Result != abci.ResponseOfferSnapshot_ABORT {
		t.Fatal(res, err)
	}
	// Refused before the application: no child exists here.
	b := application{}
	for _, bad := range []*abci.RequestOfferSnapshot{
		{AppHash: req.AppHash},
		{Snapshot: req.Snapshot, AppHash: []byte{1}},
		{Snapshot: &abci.Snapshot{Metadata: make([]byte, maxSnapshotMetadata+1)}, AppHash: req.AppHash},
	} {
		if res, err := b.OfferSnapshot(context.Background(), bad); err != nil || res.Result != abci.ResponseOfferSnapshot_REJECT {
			t.Fatal(res, err)
		}
	}
}

// A chunk to fetch again names itself and its sender; an oversized chunk
// is fetched again without reaching the application.
func TestApplySnapshotChunkMapsResultsAndRefetches(t *testing.T) {
	req := &abci.RequestApplySnapshotChunk{Index: 3, Chunk: []byte("abc"), Sender: "peer"}
	request := `apply_snapshot_chunk {"chunk":"YWJj","index":3}`
	for result, want := range map[string]abci.ResponseApplySnapshotChunk_Result{
		"accept":          abci.ResponseApplySnapshotChunk_ACCEPT,
		"retry":           abci.ResponseApplySnapshotChunk_RETRY,
		"reject_snapshot": abci.ResponseApplySnapshotChunk_REJECT_SNAPSHOT,
		"abort":           abci.ResponseApplySnapshotChunk_ABORT,
		"other":           abci.ResponseApplySnapshotChunk_ABORT,
	} {
		a := restoreChild(t, request, result)
		res, err := a.ApplySnapshotChunk(context.Background(), req)
		if err != nil || res.Result != want {
			t.Fatalf("%s: %v %v", result, res, err)
		}
		if want == abci.ResponseApplySnapshotChunk_RETRY &&
			(len(res.RefetchChunks) != 1 || res.RefetchChunks[0] != 3 || len(res.RejectSenders) != 1 || res.RejectSenders[0] != "peer") {
			t.Fatal(res)
		}
	}
	b := application{}
	res, err := b.ApplySnapshotChunk(context.Background(), &abci.RequestApplySnapshotChunk{Index: 1, Chunk: make([]byte, maxSnapshotChunk+1)})
	if err != nil || res.Result != abci.ResponseApplySnapshotChunk_RETRY || res.RefetchChunks[0] != 1 || res.RejectSenders != nil {
		t.Fatal(res, err)
	}
}
