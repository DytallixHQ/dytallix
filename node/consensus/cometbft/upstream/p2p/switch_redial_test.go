package p2p

import (
	"testing"
	"time"

	"github.com/cometbft/cometbft/config"
)

// A pinned peer is redialed forever, at most PersistentPeersMaxDialPeriod
// apart (Dytallix, P01, 2 October 2026); any other peer keeps the upstream
// schedule and gives up after reconnectBackOffAttempts.
func TestRedialWaitKeepsPersistentPeersWithinTheCap(t *testing.T) {
	cfg := config.DefaultP2PConfig()
	cfg.PersistentPeersMaxDialPeriod = time.Minute
	sw := &Switch{config: cfg}
	for attempt, want := range map[int]time.Duration{1: 3 * time.Second, 2: 9 * time.Second, 3: 27 * time.Second, 4: time.Minute, 10: time.Minute, 1000: time.Minute} {
		wait, ok := sw.redialWait(true, attempt)
		if !ok || wait != want {
			t.Fatalf("persistent attempt %d: %v %v, want %v", attempt, wait, ok, want)
		}
	}
	// Any other peer: the uncapped upstream schedule, then it gives up.
	if wait, ok := sw.redialWait(false, 10); !ok || wait != 59049*time.Second {
		t.Fatalf("last upstream step: %v %v", wait, ok)
	}
	if _, ok := sw.redialWait(false, reconnectBackOffAttempts+1); ok {
		t.Fatal("a non-persistent peer is redialed past the upstream schedule")
	}
	// Without a cap a persistent peer stays at the last upstream step.
	sw.config.PersistentPeersMaxDialPeriod = 0
	if wait, ok := sw.redialWait(true, 1_000_000); !ok || wait != 59049*time.Second {
		t.Fatalf("uncapped persistent wait: %v %v", wait, ok)
	}
}
