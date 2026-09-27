package metricsfile

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestRegistryWritesTheTextFormat(t *testing.T) {
	r := NewRegistry()
	r.Counter("b_total").Add(2)
	r.Counter("b_total").Add(3)
	r.Gauge("a_height").Set(7)
	r.Gauge("a_height").With("validator_address", `x"y`).Set(1)
	h := r.Histogram("c_seconds").With("step", "propose")
	h.Observe(0.5)
	h.Observe(1.5)
	var b bytes.Buffer
	if err := r.Write(&b, "engine", time.UnixMilli(1_700_000_000_250)); err != nil {
		t.Fatal(err)
	}
	want := `# TYPE a_height gauge
a_height 7
a_height{validator_address="x\"y"} 1
# TYPE b_total counter
b_total 5
# TYPE c_seconds summary
c_seconds_sum{step="propose"} 2
c_seconds_count{step="propose"} 2
# TYPE dytallix_metrics_written_timestamp_seconds gauge
dytallix_metrics_written_timestamp_seconds{process="engine"} 1.70000000025e+09
`
	if b.String() != want {
		t.Fatalf("got\n%s\nwant\n%s", b.String(), want)
	}
}

func TestWriteFileReplacesAtomically(t *testing.T) {
	dir := t.TempDir()
	r := NewRegistry()
	r.Gauge("x").Set(1)
	for i := 0; i < 2; i++ {
		if err := r.WriteFile(dir, "dytallix-engine.prom", "engine", time.Now()); err != nil {
			t.Fatal(err)
		}
	}
	entries, _ := os.ReadDir(dir)
	if len(entries) != 1 || entries[0].Name() != "dytallix-engine.prom" {
		t.Fatal(entries)
	}
	info, _ := os.Stat(filepath.Join(dir, "dytallix-engine.prom"))
	if info.Mode().Perm() != 0o644 {
		t.Fatal(info.Mode())
	}
	for _, bad := range [][2]string{{"relative", "a.prom"}, {dir, "../a.prom"}, {dir, "a.txt"}} {
		if err := r.WriteFile(bad[0], bad[1], "engine", time.Now()); err == nil {
			t.Fatal("accepted", bad)
		}
	}
}

// The core set is recorded; every other CometBFT metric stays a no-op.
func TestProviderRecordsTheCoreSetOnly(t *testing.T) {
	r := NewRegistry()
	consensus, peers, mempool, state, _, blocks, snapshots := Provider(r)("chain")
	consensus.Height.Set(12)
	consensus.ValidatorMissedBlocks.With("validator_address", "AB").Set(1)
	peers.Peers.Set(3)
	mempool.Size.Set(4)
	blocks.Syncing.Set(0)
	snapshots.Syncing.Set(1)
	if _, ok := consensus.NumTxs.(*gauge); ok {
		t.Fatal("a metric outside the core set is recorded")
	}
	if _, ok := peers.PeerSendBytesTotal.(*counter); ok {
		t.Fatal("a per-peer metric is recorded")
	}
	if _, ok := state.BlockProcessingTime.(*histogram); ok {
		t.Fatal("state metrics are recorded")
	}
	var b bytes.Buffer
	if err := r.Write(&b, "engine", time.Now()); err != nil {
		t.Fatal(err)
	}
	for _, line := range []string{
		"dytallix_engine_consensus_height 12",
		`dytallix_engine_consensus_validator_missed_blocks{validator_address="AB"} 1`,
		"dytallix_engine_p2p_peers 3",
		"dytallix_engine_mempool_size 4",
		"dytallix_engine_statesync_syncing 1",
		// Present before any update.
		"dytallix_engine_mempool_failed_txs 0",
		"dytallix_engine_blocksync_latest_block_height 0",
	} {
		if !strings.Contains(b.String(), line+"\n") {
			t.Fatalf("missing %q in\n%s", line, b.String())
		}
	}
}
