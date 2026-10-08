package lightblocks

// Snapshot light blocks (disaster recovery v1, step R4; P01, 7 October 2026).
// The sentry's engine writes the light blocks a restore from application
// snapshot H verifies, heights H to H+2, once H+2 commits, so each off-host
// copy carries its own headers and no node is stopped to export them.

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/cometbft/cometbft/libs/log"
)

// SnapshotSpan is the number of heights after a snapshot's own that its light
// blocks hold: header H+1 carries the application hash of H, and H+2 commits
// it.
const SnapshotSpan = 2

// SnapshotStores is the part of a running node's block store the writer
// reads.
type SnapshotStores interface {
	BlockReader
	Height() int64
}

// SnapshotWriter publishes one directory per due height H under Dir, named
// {H:020}, holding the export of heights H to H+2. A height is due when it is
// a multiple of Interval, the application's snapshot interval. The latest Keep
// directories are kept, as the application keeps its snapshots.
type SnapshotWriter struct {
	dir      string
	interval int64
	keep     int
	chainID  string
	blocks   SnapshotStores
	states   StateReader
	// failed is the last due height whose export failed; it is not retried.
	failed int64
}

// NewSnapshotWriter checks the output directory, an existing absolute
// directory without group or other write, and removes staging directories an
// interrupted writer left.
func NewSnapshotWriter(dir string, interval int64, keep int, chainID string, blocks SnapshotStores, states StateReader) (*SnapshotWriter, error) {
	if !filepath.IsAbs(dir) || interval < 1 || keep < 1 || chainID == "" {
		return nil, errors.New("snapshot light blocks need an absolute directory, an interval and a count to keep")
	}
	info, err := os.Lstat(dir)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() || info.Mode().Perm()&0o022 != 0 {
		return nil, fmt.Errorf("%s must be a directory without group or other write", dir)
	}
	w := &SnapshotWriter{dir: dir, interval: interval, keep: keep, chainID: chainID, blocks: blocks, states: states}
	return w, w.removePartial()
}

func snapshotName(height int64) string { return fmt.Sprintf("%020d", height) }

// due is the newest due height whose last light block has committed, or 0.
func (w *SnapshotWriter) due() int64 {
	top := w.blocks.Height() - SnapshotSpan
	if top < w.interval {
		return 0
	}
	return top - top%w.interval
}

// Once publishes the newest due height's light blocks unless they are
// published already, and returns that height, or 0 when it wrote nothing.
func (w *SnapshotWriter) Once() (int64, error) {
	height := w.due()
	if height == 0 || height == w.failed {
		return 0, nil
	}
	final := filepath.Join(w.dir, snapshotName(height))
	if _, err := os.Lstat(final); err == nil {
		return 0, nil
	} else if !errors.Is(err, fs.ErrNotExist) {
		return 0, err
	}
	staging := filepath.Join(w.dir, ".staging-"+snapshotName(height))
	if err := os.RemoveAll(staging); err != nil {
		return 0, err
	}
	if err := os.Mkdir(staging, 0o700); err != nil {
		return 0, err
	}
	if err := Export(w.blocks, w.states, w.chainID, height, height+SnapshotSpan, staging); err != nil {
		os.RemoveAll(staging)
		w.failed = height
		return 0, fmt.Errorf("snapshot light blocks at %d: %w", height, err)
	}
	if err := syncDirectory(staging); err != nil {
		os.RemoveAll(staging)
		return 0, err
	}
	if err := os.Rename(staging, final); err != nil {
		os.RemoveAll(staging)
		return 0, err
	}
	if err := syncDirectory(w.dir); err != nil {
		return height, err
	}
	return height, w.prune()
}

// Published lists the published heights, ascending.
func (w *SnapshotWriter) Published() ([]int64, error) {
	entries, err := os.ReadDir(w.dir)
	if err != nil {
		return nil, err
	}
	var heights []int64
	for _, entry := range entries {
		name := entry.Name()
		if !entry.IsDir() || len(name) != 20 || strings.Trim(name, "0123456789") != "" {
			continue
		}
		height, err := strconv.ParseInt(name, 10, 64)
		if err != nil {
			continue
		}
		heights = append(heights, height)
	}
	sort.Slice(heights, func(i, j int) bool { return heights[i] < heights[j] })
	return heights, nil
}

// prune removes all but the latest keep published directories.
func (w *SnapshotWriter) prune() error {
	heights, err := w.Published()
	if err != nil {
		return err
	}
	for len(heights) > w.keep {
		if err := os.RemoveAll(filepath.Join(w.dir, snapshotName(heights[0]))); err != nil {
			return err
		}
		heights = heights[1:]
	}
	return nil
}

// removePartial removes the staging directories an interrupted writer left.
func (w *SnapshotWriter) removePartial() error {
	entries, err := os.ReadDir(w.dir)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".staging-") {
			if err := os.RemoveAll(filepath.Join(w.dir, entry.Name())); err != nil {
				return err
			}
		}
	}
	return nil
}

// Run calls Once every tick until ctx ends. A failure is logged and never
// affects the node.
func (w *SnapshotWriter) Run(ctx context.Context, tick time.Duration, logger log.Logger) {
	ticker := time.NewTicker(tick)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
		height, err := w.Once()
		if err != nil {
			logger.Error("Snapshot light blocks not written", "err", err)
		} else if height > 0 {
			logger.Info("Wrote snapshot light blocks", "height", height, "to", height+SnapshotSpan)
		}
	}
}

func syncDirectory(path string) error {
	dir, err := os.Open(path)
	if err != nil {
		return err
	}
	defer dir.Close()
	return dir.Sync()
}
