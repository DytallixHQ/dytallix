package main

import (
	"bytes"
	"context"
	"crypto/sha3"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"

	abci "github.com/cometbft/cometbft/abci/types"
)

// Snapshot files written by the application (state sync v1, rule 3). The
// bridge lists and serves them without calling the application, so serving
// a peer never waits on the application lock. A joining node checks every
// chunk against the metadata, so these files are trusted for availability
// only.
const (
	snapshotFormat      = 1
	maxSnapshotChunk    = 4 << 20
	maxSnapshotMetadata = 1 << 20
	metadataFile        = "metadata.json"
)

type snapshotStore struct{ dir string }

type snapshotMetadata struct {
	Format       uint32   `json:"format"`
	ChainID      string   `json:"chain_id"`
	Height       uint64   `json:"height"`
	AppHash      string   `json:"app_hash"`
	StateDigest  string   `json:"state_digest"`
	RetainedFrom uint64   `json:"retained_from"`
	Entries      uint64   `json:"entries"`
	Bytes        uint64   `json:"bytes"`
	Chunks       []string `json:"chunks"`
}

func snapshotName(height uint64) string { return fmt.Sprintf("%020d", height) }

func readLimited(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limit {
		return nil, errors.New("snapshot file exceeds its bound")
	}
	return data, nil
}

// metadata reads a published snapshot's metadata and its raw bytes.
func (s snapshotStore) metadata(height uint64) (*snapshotMetadata, []byte, error) {
	raw, err := readLimited(filepath.Join(s.dir, snapshotName(height), metadataFile), maxSnapshotMetadata)
	if err != nil {
		return nil, nil, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var m snapshotMetadata
	if err := decoder.Decode(&m); err != nil {
		return nil, nil, err
	}
	if decoder.More() || m.Format != snapshotFormat || m.Height != height || len(m.Chunks) == 0 || uint64(len(m.Chunks)) > uint64(^uint32(0)) {
		return nil, nil, errors.New("invalid snapshot metadata")
	}
	return &m, raw, nil
}

// list returns the published snapshots, newest first. Anything unreadable
// is not offered: a snapshot being removed, or a missing directory. An ABCI
// error here would stop the engine's application connection.
func (s snapshotStore) list() []*abci.Snapshot {
	if s.dir == "" {
		return nil
	}
	entries, err := os.ReadDir(s.dir)
	if err != nil {
		return nil
	}
	var heights []uint64
	for _, entry := range entries {
		name := entry.Name()
		if len(name) != 20 || !entry.IsDir() {
			continue
		}
		height, err := strconv.ParseUint(name, 10, 64)
		if err != nil || snapshotName(height) != name {
			continue
		}
		heights = append(heights, height)
	}
	sort.Slice(heights, func(i, j int) bool { return heights[i] > heights[j] })
	var snapshots []*abci.Snapshot
	for _, height := range heights {
		m, raw, err := s.metadata(height)
		if err != nil {
			continue
		}
		hash := sha3.Sum256(raw)
		snapshots = append(snapshots, &abci.Snapshot{
			Height: height, Format: snapshotFormat, Chunks: uint32(len(m.Chunks)),
			Hash: hash[:], Metadata: raw,
		})
	}
	return snapshots
}

// chunk returns one chunk of a published snapshot, or nil when it is not
// available; the engine then reports it missing to the peer.
func (s snapshotStore) chunk(height uint64, format uint32, index uint32) []byte {
	if s.dir == "" || format != snapshotFormat {
		return nil
	}
	m, _, err := s.metadata(height)
	if err != nil || uint64(index) >= uint64(len(m.Chunks)) {
		return nil
	}
	path := filepath.Join(s.dir, snapshotName(height), fmt.Sprintf("chunk-%06d", index))
	data, err := readLimited(path, maxSnapshotChunk)
	if err != nil || len(data) == 0 {
		return nil
	}
	return data
}

func (a *application) ListSnapshots(context.Context, *abci.RequestListSnapshots) (*abci.ResponseListSnapshots, error) {
	return &abci.ResponseListSnapshots{Snapshots: a.snapshots.list()}, nil
}

func (a *application) LoadSnapshotChunk(_ context.Context, req *abci.RequestLoadSnapshotChunk) (*abci.ResponseLoadSnapshotChunk, error) {
	return &abci.ResponseLoadSnapshotChunk{Chunk: a.snapshots.chunk(req.Height, req.Format, req.Chunk)}, nil
}

// OfferSnapshot passes a snapshot and the application hash the light client
// verified for its height to the application (state sync v1, rule 4). A
// failed call aborts state sync: the application's state is unknown.
func (a *application) OfferSnapshot(ctx context.Context, req *abci.RequestOfferSnapshot) (*abci.ResponseOfferSnapshot, error) {
	if req.Snapshot == nil || len(req.AppHash) != 32 || len(req.Snapshot.Metadata) > maxSnapshotMetadata {
		return &abci.ResponseOfferSnapshot{Result: abci.ResponseOfferSnapshot_REJECT}, nil
	}
	payload := map[string]any{
		"height": req.Snapshot.Height, "format": req.Snapshot.Format, "chunks": req.Snapshot.Chunks,
		"hash":     base64.StdEncoding.EncodeToString(req.Snapshot.Hash),
		"metadata": base64.StdEncoding.EncodeToString(req.Snapshot.Metadata),
		"app_hash": hex.EncodeToString(req.AppHash),
	}
	var result struct {
		Result string `json:"result"`
	}
	if err := a.child.call(ctx, "offer_snapshot", payload, &result); err != nil {
		return &abci.ResponseOfferSnapshot{Result: abci.ResponseOfferSnapshot_ABORT}, nil
	}
	outcome, ok := map[string]abci.ResponseOfferSnapshot_Result{
		"accept":        abci.ResponseOfferSnapshot_ACCEPT,
		"reject":        abci.ResponseOfferSnapshot_REJECT,
		"reject_format": abci.ResponseOfferSnapshot_REJECT_FORMAT,
		"abort":         abci.ResponseOfferSnapshot_ABORT,
	}[result.Result]
	if !ok {
		outcome = abci.ResponseOfferSnapshot_ABORT
	}
	return &abci.ResponseOfferSnapshot{Result: outcome}, nil
}

// ApplySnapshotChunk passes one chunk of the accepted snapshot to the
// application. A chunk that fails its hash, or is too large to be one, is
// fetched again from another sender.
func (a *application) ApplySnapshotChunk(ctx context.Context, req *abci.RequestApplySnapshotChunk) (*abci.ResponseApplySnapshotChunk, error) {
	retry := &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_RETRY, RefetchChunks: []uint32{req.Index}}
	if req.Sender != "" {
		retry.RejectSenders = []string{req.Sender}
	}
	if len(req.Chunk) > maxSnapshotChunk {
		return retry, nil
	}
	payload := map[string]any{"index": req.Index, "chunk": base64.StdEncoding.EncodeToString(req.Chunk)}
	var result struct {
		Result string `json:"result"`
	}
	if err := a.child.call(ctx, "apply_snapshot_chunk", payload, &result); err != nil {
		return &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_ABORT}, nil
	}
	switch result.Result {
	case "accept":
		return &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_ACCEPT}, nil
	case "retry":
		return retry, nil
	case "reject_snapshot":
		return &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_REJECT_SNAPSHOT}, nil
	default:
		return &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_ABORT}, nil
	}
}
