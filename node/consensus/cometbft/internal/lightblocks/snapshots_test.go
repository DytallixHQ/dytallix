package lightblocks

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// heightStores is a node holding c's blocks up to height.
type heightStores struct {
	fakeStores
	height int64
}

func (h *heightStores) Height() int64 { return h.height }

func names(t *testing.T, dir string) []string {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	var out []string
	for _, entry := range entries {
		out = append(out, entry.Name())
	}
	return out
}

func TestSnapshotWriterPublishesEachDueHeightsLightBlocks(t *testing.T) {
	c := newChain(t, 45, 25, appHash)
	dir := t.TempDir()
	stores := &heightStores{fakeStores{c}, 11}
	w, err := NewSnapshotWriter(dir, 10, 2, testChain, stores, fakeStores{c})
	if err != nil {
		t.Fatal(err)
	}
	// Height 10's light blocks need 12 committed.
	if height, err := w.Once(); height != 0 || err != nil {
		t.Fatal(height, err)
	}
	stores.height = 12
	if height, err := w.Once(); height != 10 || err != nil {
		t.Fatal(height, err)
	}
	want := []string{"00000000000000000010.block", "00000000000000000010.params", "00000000000000000011.block",
		"00000000000000000011.params", "00000000000000000012.block", "00000000000000000012.params"}
	if got := names(t, filepath.Join(dir, snapshotName(10))); len(got) != len(want) {
		t.Fatal(got)
	} else {
		for i := range want {
			if got[i] != want[i] {
				t.Fatal(got)
			}
		}
	}
	// Published once: the next tick writes nothing.
	if height, err := w.Once(); height != 0 || err != nil {
		t.Fatal(height, err)
	}
	// A restore trusting the header at 10 verifies the snapshot's height.
	sp, err := stateProvider(t, c, 10, filepath.Join(dir, snapshotName(10)))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.State(context.Background(), 10); err != nil {
		t.Fatal(err)
	}
	// Later due heights, across the validator set change; the latest two stay.
	for _, step := range [][2]int64{{29, 20}, {32, 30}} {
		stores.height = step[0]
		if height, err := w.Once(); height != step[1] || err != nil {
			t.Fatal(step, height, err)
		}
	}
	if heights, err := w.Published(); err != nil || len(heights) != 2 || heights[0] != 20 || heights[1] != 30 {
		t.Fatal(heights, err)
	}
}

func TestSnapshotWriterRemovesPartialOutputAndDoesNotRetryAFailure(t *testing.T) {
	c := newChain(t, 12, 20, appHash)
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, ".staging-00000000000000000010"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "other"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	// The store claims a height whose blocks it does not hold.
	stores := &heightStores{fakeStores{c}, 22}
	w, err := NewSnapshotWriter(dir, 10, 2, testChain, stores, fakeStores{c})
	if err != nil {
		t.Fatal(err)
	}
	if got := names(t, dir); len(got) != 1 || got[0] != "other" {
		t.Fatal("staging output survived:", got)
	}
	if height, err := w.Once(); height != 0 || err == nil {
		t.Fatal("exported heights the node does not hold", height)
	}
	if got := names(t, dir); len(got) != 1 {
		t.Fatal("a failed export left output:", got)
	}
	if height, err := w.Once(); height != 0 || err != nil {
		t.Fatal("retried a failed height", height, err)
	}
}

func TestSnapshotWriterRefusesUnsafeSettings(t *testing.T) {
	c := newChain(t, 3, 10, appHash)
	stores := &heightStores{fakeStores{c}, 3}
	open := t.TempDir()
	if err := os.Chmod(open, 0o777); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []struct {
		dir      string
		interval int64
		keep     int
	}{{"relative", 10, 2}, {t.TempDir(), 0, 2}, {t.TempDir(), 10, 0}, {open, 10, 2}, {filepath.Join(t.TempDir(), "missing"), 10, 2}} {
		if _, err := NewSnapshotWriter(bad.dir, bad.interval, bad.keep, testChain, stores, fakeStores{c}); err == nil {
			t.Fatal("accepted", bad)
		}
	}
}
