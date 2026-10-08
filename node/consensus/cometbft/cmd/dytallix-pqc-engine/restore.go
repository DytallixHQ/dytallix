package main

// A restore from an opened off-host copy (disaster recovery v1, R5; P01, 7
// and 8 October 2026): the snapshot's files, with its light blocks under
// light-blocks/, as backup-open writes them. The node must hold no state.

import (
	"context"
	"crypto/sha3"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/lightblocks"

	"github.com/cometbft/cometbft/statesync"
)

const (
	restoreMetadata    = "metadata.json"
	restoreLightBlocks = "light-blocks"
	maxRestoreMetadata = 16 << 20
	// The application's chunk bound (state sync v1, C2).
	maxRestoreChunk = 4 << 20
	// A restore trusts the copy's header for the evidence age less one day
	// (P01, 8 October 2026): 13 days with the approved 14.
	restoreTrustMargin = 24 * time.Hour
)

// copyMetadata is the part of the snapshot's metadata the engine reads; the
// application checks all of it when the snapshot is offered.
type copyMetadata struct {
	Format  uint32   `json:"format"`
	ChainID string   `json:"chain_id"`
	Height  uint64   `json:"height"`
	Chunks  []string `json:"chunks"`
}

func readBounded(path string, limit int64) ([]byte, error) {
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
		return nil, fmt.Errorf("%s exceeds its bound", path)
	}
	return data, nil
}

// readCopy reads the copy's metadata and returns the local snapshot it
// describes.
func readCopy(dir, chainID string) (statesync.LocalSnapshot, error) {
	if !filepath.IsAbs(dir) || filepath.Clean(dir) != dir {
		return statesync.LocalSnapshot{}, errors.New("--restore-snapshot must be a clean absolute path")
	}
	raw, err := readBounded(filepath.Join(dir, restoreMetadata), maxRestoreMetadata)
	if err != nil {
		return statesync.LocalSnapshot{}, err
	}
	var meta copyMetadata
	if err = json.Unmarshal(raw, &meta); err != nil {
		return statesync.LocalSnapshot{}, fmt.Errorf("the copy's metadata: %w", err)
	}
	if meta.ChainID != chainID || meta.Height == 0 || len(meta.Chunks) == 0 || len(meta.Chunks) > 1<<16 {
		return statesync.LocalSnapshot{}, fmt.Errorf("the copy is not a snapshot of %s", chainID)
	}
	// The bridge offers a snapshot's metadata bytes with their SHA3-256 as
	// its hash; the application checks the same.
	hash := sha3.Sum256(raw)
	return statesync.LocalSnapshot{
		Height:   meta.Height,
		Format:   meta.Format,
		Chunks:   uint32(len(meta.Chunks)),
		Hash:     hash[:],
		Metadata: raw,
		Chunk: func(index uint32) ([]byte, error) {
			return readBounded(filepath.Join(dir, fmt.Sprintf("chunk-%06d", index)), maxRestoreChunk)
		},
	}, nil
}

// restoreOption sets the node's state sync to a one-time restore of the copy
// in dir and returns its light blocks' directory. The light client trusts the
// copy's own header at its height, which the backup code authenticated, and
// verifies the two after it.
func restoreOption(ctx context.Context, runtime *enginepqc.Runtime, dir string) (statesync.LocalSnapshot, string, error) {
	local, err := readCopy(dir, runtime.Genesis.ChainID)
	if err != nil {
		return local, "", err
	}
	light := filepath.Join(dir, restoreLightBlocks)
	provider, err := lightblocks.NewProvider(runtime.Genesis.ChainID, light)
	if err != nil {
		return local, "", err
	}
	trusted, err := provider.LightBlock(ctx, int64(local.Height))
	if err != nil {
		return local, "", fmt.Errorf("the copy's light block at %d: %w", local.Height, err)
	}
	evidence := runtime.Genesis.ConsensusParams.Evidence.MaxAgeDuration
	if evidence <= restoreTrustMargin {
		return local, "", errors.New("the evidence age leaves no restore trust period")
	}
	sync := runtime.Config.StateSync
	sync.Enable = true
	sync.LocalRestore = true
	sync.TrustHeight = int64(local.Height)
	sync.TrustHash = hex.EncodeToString(trusted.Hash())
	sync.TrustPeriod = evidence - restoreTrustMargin
	// The chunk queue lives under the node's own data, a writable root.
	sync.TempDir = runtime.Config.DBDir()
	return local, light, nil
}
