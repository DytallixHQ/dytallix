package statesync

import (
	"errors"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/proxy"
	proxymocks "github.com/cometbft/cometbft/proxy/mocks"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/statesync/mocks"
	"github.com/cometbft/cometbft/types"
)

// localSource is a three-chunk copy that counts its reads.
type localSource struct {
	mtx   sync.Mutex
	reads map[uint32]int
	fail  uint32
}

func (s *localSource) chunk(index uint32) ([]byte, error) {
	s.mtx.Lock()
	defer s.mtx.Unlock()
	s.reads[index]++
	if index == s.fail {
		return nil, errors.New("unreadable")
	}
	return []byte{1, 1, byte(index)}, nil
}

func (s *localSource) count(index uint32) int {
	s.mtx.Lock()
	defer s.mtx.Unlock()
	return s.reads[index]
}

func localSyncer(t *testing.T, appHeight int64) (*syncer, *proxymocks.AppConnSnapshot, sm.State) {
	t.Helper()
	state := sm.State{ChainID: "chain", LastBlockHeight: 1, AppHash: []byte("app_hash")}
	state.Version.Consensus.App = testAppVersion
	stateProvider := &mocks.StateProvider{}
	stateProvider.On("AppHash", mock.Anything, uint64(1)).Return(state.AppHash, nil)
	stateProvider.On("State", mock.Anything, uint64(1)).Return(state, nil)
	stateProvider.On("Commit", mock.Anything, uint64(1)).Return(&types.Commit{}, nil)
	connSnapshot := &proxymocks.AppConnSnapshot{}
	connSnapshot.On("OfferSnapshot", mock.Anything, &abci.RequestOfferSnapshot{
		Snapshot: &abci.Snapshot{Height: 1, Format: 1, Chunks: 3, Hash: []byte{1, 2, 3}, Metadata: []byte("metadata")},
		AppHash:  []byte("app_hash"),
	}).Return(&abci.ResponseOfferSnapshot{Result: abci.ResponseOfferSnapshot_ACCEPT}, nil)
	connQuery := &proxymocks.AppConnQuery{}
	connQuery.On("Info", mock.Anything, proxy.RequestInfo).Return(&abci.ResponseInfo{
		AppVersion: testAppVersion, LastBlockHeight: appHeight, LastBlockAppHash: []byte("app_hash"),
	}, nil)
	cfg := *config.DefaultStateSyncConfig()
	cfg.ChunkFetchers = 0
	return newSyncer(cfg, log.NewNopLogger(), connSnapshot, connQuery, stateProvider, ""), connSnapshot, state
}

func local(source *localSource) LocalSnapshot {
	return LocalSnapshot{Height: 1, Format: 1, Chunks: 3, Hash: []byte{1, 2, 3}, Metadata: []byte("metadata"),
		Chunk: source.chunk}
}

func apply(conn *proxymocks.AppConnSnapshot, index uint32, result abci.ResponseApplySnapshotChunk_Result, refetch ...uint32) *mock.Call {
	return conn.On("ApplySnapshotChunk", mock.Anything, &abci.RequestApplySnapshotChunk{
		Index: index, Chunk: []byte{1, 1, byte(index)},
	}).Return(&abci.ResponseApplySnapshotChunk{Result: result, RefetchChunks: refetch}, nil)
}

func TestRestoreLocalAppliesEachChunkAndReadsARefetchOnce(t *testing.T) {
	syncer, conn, want := localSyncer(t, 1)
	apply(conn, 0, abci.ResponseApplySnapshotChunk_ACCEPT).Once()
	// The application asks for chunk 1 again once.
	apply(conn, 1, abci.ResponseApplySnapshotChunk_RETRY, 1).Once()
	apply(conn, 1, abci.ResponseApplySnapshotChunk_ACCEPT).Once()
	apply(conn, 2, abci.ResponseApplySnapshotChunk_ACCEPT).Once()
	source := &localSource{reads: map[uint32]int{}, fail: 99}
	state, commit, err := restoreLocal(syncer, local(source), "", log.NewNopLogger())
	require.NoError(t, err)
	require.Equal(t, want, state)
	require.NotNil(t, commit)
	require.Equal(t, []int{1, 2, 1}, []int{source.count(0), source.count(1), source.count(2)})
	conn.AssertExpectations(t)
}

func TestRestoreLocalFailsOnARepeatedRefetchOrAnUnreadableChunk(t *testing.T) {
	// A copy does not change: a third request for one chunk fails the restore.
	syncer, conn, _ := localSyncer(t, 0)
	apply(conn, 0, abci.ResponseApplySnapshotChunk_RETRY, 0)
	source := &localSource{reads: map[uint32]int{}, fail: 99}
	done := make(chan error, 1)
	go func() {
		_, _, err := restoreLocal(syncer, local(source), "", log.NewNopLogger())
		done <- err
	}()
	select {
	case err := <-done:
		require.Error(t, err)
	case <-time.After(30 * time.Second):
		t.Fatal("a repeated refetch did not end the restore")
	}
	require.Equal(t, maxLocalReads, source.count(0))

	// An unreadable chunk fails it too.
	syncer, conn, _ = localSyncer(t, 0)
	apply(conn, 0, abci.ResponseApplySnapshotChunk_ACCEPT).Maybe()
	source = &localSource{reads: map[uint32]int{}, fail: 1}
	_, _, err := restoreLocal(syncer, local(source), "", log.NewNopLogger())
	require.Error(t, err)
}

// A reactor's syncs, from peers or from a local copy, keep their chunks in
// the configured directory: a production engine names one in its writable
// data root, and the system temp directory is not one.
func TestReactorKeepsChunksInTheConfiguredDirectory(t *testing.T) {
	dir := t.TempDir()
	cfg := *config.DefaultStateSyncConfig()
	cfg.TempDir = dir
	r := NewReactor(cfg, nil, nil, NopMetrics())
	queue, err := newChunkQueue(&snapshot{Height: 1, Format: 1, Chunks: 1}, r.tempDir)
	require.NoError(t, err)
	defer queue.Close()
	require.Equal(t, dir, filepath.Dir(queue.dir))
}

func TestRestoreLocalNeedsItsChunks(t *testing.T) {
	r := &Reactor{}
	_, _, err := r.RestoreLocal(&mocks.StateProvider{}, LocalSnapshot{Height: 1, Chunks: 3})
	require.Error(t, err)
	_, _, err = r.RestoreLocal(&mocks.StateProvider{}, LocalSnapshot{Height: 1, Chunk: func(uint32) ([]byte, error) { return nil, nil }})
	require.Error(t, err)
}
