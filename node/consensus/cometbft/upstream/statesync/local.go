package statesync

// Dytallix: a restore from a local copy of a snapshot (disaster recovery v1,
// R5; P01, 7 October 2026). The chunks come from a directory instead of
// peers. Every other step is the state sync's own: the light client's trusted
// application hash, the offer, the chunks the application checks, and the
// application's height and hash afterwards.

import (
	"context"
	"errors"
	"time"

	"github.com/cometbft/cometbft/libs/log"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/types"
)

// LocalSnapshot is a snapshot read from a local copy.
type LocalSnapshot struct {
	Height   uint64
	Format   uint32
	Chunks   uint32
	Hash     []byte
	Metadata []byte
	// Chunk reads one chunk of the copy.
	Chunk func(index uint32) ([]byte, error)
}

// maxLocalReads bounds how often one chunk is read: once, and once more if
// the application asks for it again. A copy does not change, so a third
// request fails the restore.
const maxLocalReads = 2

// RestoreLocal restores a local snapshot through the same steps as a sync
// from peers, without asking a peer for anything. Snapshots and chunks that
// peers send meanwhile are ignored.
func (r *Reactor) RestoreLocal(stateProvider StateProvider, local LocalSnapshot) (sm.State, *types.Commit, error) {
	if local.Chunk == nil || local.Chunks == 0 {
		return sm.State{}, nil, errors.New("a local snapshot needs its chunks")
	}
	r.mtx.RLock()
	busy := r.syncer != nil
	r.mtx.RUnlock()
	if busy {
		return sm.State{}, nil, errors.New("a state sync is already in progress")
	}
	cfg := r.cfg
	cfg.ChunkFetchers = 0
	syncer := newSyncer(cfg, r.Logger, r.conn, r.connQuery, stateProvider, r.tempDir)
	return restoreLocal(syncer, local, r.tempDir, r.Logger)
}

func restoreLocal(syncer *syncer, local LocalSnapshot, tempDir string, logger log.Logger) (sm.State, *types.Commit, error) {
	snap := &snapshot{Height: local.Height, Format: local.Format, Chunks: local.Chunks, Hash: local.Hash,
		Metadata: local.Metadata}
	queue, err := newChunkQueue(snap, tempDir)
	if err != nil {
		return sm.State{}, nil, err
	}
	defer queue.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go readLocalChunks(ctx, queue, snap, local.Chunk, logger)
	return syncer.Sync(snap, queue)
}

// readLocalChunks adds each chunk the queue allocates, read from the copy. A
// chunk the application discards is allocated again and read once more.
// Closing the queue on a failure ends the restore: the application then
// holds no complete state, and the sync's final check refuses it.
func readLocalChunks(ctx context.Context, queue *chunkQueue, snap *snapshot, read func(uint32) ([]byte, error), logger log.Logger) {
	reads := make(map[uint32]int)
	for ctx.Err() == nil {
		index, err := queue.Allocate()
		if errors.Is(err, errDone) {
			select {
			case <-ctx.Done():
				return
			case <-time.After(100 * time.Millisecond):
			}
			continue
		}
		reads[index]++
		if reads[index] > maxLocalReads {
			logger.Error("Local snapshot chunk refused again", "chunk", index)
			queue.Close()
			return
		}
		data, err := read(index)
		if err == nil {
			_, err = queue.Add(&chunk{Height: snap.Height, Format: snap.Format, Index: index, Chunk: data})
		}
		if err != nil {
			logger.Error("Local snapshot chunk unreadable", "chunk", index, "err", err)
			queue.Close()
			return
		}
	}
}
