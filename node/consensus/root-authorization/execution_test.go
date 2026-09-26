package rootauthorization

import (
	"bytes"
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
)

var errFixtureWrite = errors.New("fixture storage failure")

// diskFixture is test-only. The mutex coordinates this test process. It is not
// a production store, multi-process lock or power-loss qualification artifact.
type diskFixture struct {
	mu                  sync.Mutex
	path                string
	failBeforeWrite     bool
	loseAcknowledgement bool
}

func (s *diskFixture) read() (ExecutionState, error) {
	var current ExecutionState
	b, err := os.ReadFile(s.path)
	if err != nil {
		return current, err
	}
	err = json.Unmarshal(b, &current)
	return current, err
}
func (s *diskFixture) Update(update func(ExecutionState) (ExecutionState, error)) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	current, err := s.read()
	if err != nil {
		return err
	}
	next, err := update(current)
	if err != nil {
		return err
	}
	if s.failBeforeWrite {
		return errFixtureWrite
	}
	b, err := json.Marshal(next)
	if err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(s.path), "root-state-")
	if err != nil {
		return err
	}
	name := f.Name()
	defer os.Remove(name)
	if _, err = f.Write(b); err != nil {
		f.Close()
		return err
	}
	if err = f.Sync(); err != nil {
		f.Close()
		return err
	}
	if err = f.Close(); err != nil {
		return err
	}
	if err = os.Rename(name, s.path); err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(s.path))
	if err != nil {
		return err
	}
	err = dir.Sync()
	dir.Close()
	if err != nil {
		return err
	}
	if s.loseAcknowledgement {
		return errFixtureWrite
	}
	return nil
}
func newDiskFixture(t *testing.T, pub []byte) *diskFixture {
	t.Helper()
	s := &diskFixture{path: filepath.Join(t.TempDir(), "state.json")}
	b, err := json.Marshal(ExecutionState{ChainID: "local-qualification", FinalizedHeight: 15, TrustedPublicKey: pub,
		Consumed: map[Action]uint64{Genesis: 0, Upgrade: 6, Emergency: 0}, Application: []byte("before")})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(s.path, b, 0600); err != nil {
		t.Fatal(err)
	}
	return s
}
func snapshot(t *testing.T, s *diskFixture) []byte {
	t.Helper()
	b, err := os.ReadFile(s.path)
	if err != nil {
		t.Fatal(err)
	}
	return b
}
func TestAtomicExecution(t *testing.T) {
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	sig, err := SignForPolicy(e, priv, p)
	if err != nil {
		t.Fatal(err)
	}
	artifact := []byte("local fixture only")
	intent := ExecutionIntent{ChainID: p.ChainID, Action: p.Action, ArtifactDigest: p.ExpectedArtifactDigest, FinalizedHeight: p.CurrentHeight}
	apply := func(current, artifact []byte) ([]byte, error) {
		if string(current) != "before" || string(artifact) != "local fixture only" {
			return nil, errors.New("fixture schema or state mismatch")
		}
		return []byte("after"), nil
	}
	t.Run("commit-restart-and-replay", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		if err := Execute(s, intent, e, sig, artifact, apply); err != nil {
			t.Fatal(err)
		}
		reopened := &diskFixture{path: s.path}
		state, err := reopened.read()
		if err != nil {
			t.Fatal(err)
		}
		if state.Consumed[Upgrade] != 7 || string(state.Application) != "after" {
			t.Fatal("action and sequence differ")
		}
		before := snapshot(t, reopened)
		if err := Execute(reopened, intent, e, sig, artifact, apply); !errors.Is(err, ErrPolicy) {
			t.Fatalf("replay: %v", err)
		}
		if !bytes.Equal(before, snapshot(t, reopened)) {
			t.Fatal("replay changed durable state")
		}
	})
	t.Run("concurrent-submit-once", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		var success, transitions atomic.Int32
		var wg sync.WaitGroup
		for i := 0; i < 8; i++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				err := Execute(s, intent, e, sig, artifact, func(a, b []byte) ([]byte, error) { transitions.Add(1); return apply(a, b) })
				if err == nil {
					success.Add(1)
				} else if !errors.Is(err, ErrPolicy) {
					t.Errorf("unexpected error: %v", err)
				}
			}()
		}
		wg.Wait()
		if success.Load() != 1 || transitions.Load() != 1 {
			t.Fatalf("success=%d transitions=%d", success.Load(), transitions.Load())
		}
		state, err := s.read()
		if err != nil || state.Consumed[Upgrade] != 7 || string(state.Application) != "after" {
			t.Fatalf("bad commit: %v", err)
		}
	})
	t.Run("failed-write-retries-from-durable-predecessor", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		before := snapshot(t, s)
		s.failBeforeWrite = true
		if err := Execute(s, intent, e, sig, artifact, apply); !errors.Is(err, errFixtureWrite) {
			t.Fatal(err)
		}
		if !bytes.Equal(before, snapshot(t, s)) {
			t.Fatal("failed write changed disk")
		}
		reopened := &diskFixture{path: s.path}
		if err := Execute(reopened, intent, e, sig, artifact, apply); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("lost-acknowledgement-does-not-apply-twice", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		s.loseAcknowledgement = true
		if err := Execute(s, intent, e, sig, artifact, apply); !errors.Is(err, errFixtureWrite) {
			t.Fatal(err)
		}
		reopened := &diskFixture{path: s.path}
		before := snapshot(t, reopened)
		if err := Execute(reopened, intent, e, sig, artifact, apply); !errors.Is(err, ErrPolicy) {
			t.Fatal(err)
		}
		if !bytes.Equal(before, snapshot(t, reopened)) {
			t.Fatal("lost acknowledgement replay changed state")
		}
	})
	t.Run("action-error-discards-mutations", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		before := snapshot(t, s)
		err := Execute(s, intent, e, sig, artifact, func(a, b []byte) ([]byte, error) { a[0] = 'X'; b[0] = 'X'; return a, errFixtureWrite })
		if !errors.Is(err, errFixtureWrite) || !bytes.Equal(before, snapshot(t, s)) || string(artifact) != "local fixture only" {
			t.Fatal("failed action leaked changes")
		}
	})
	t.Run("revocation-rechecked-inside-transaction", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		if err := s.Update(func(a ExecutionState) (ExecutionState, error) { a.Revoked = true; return a, nil }); err != nil {
			t.Fatal(err)
		}
		before := snapshot(t, s)
		if err := Execute(s, intent, e, sig, artifact, apply); !errors.Is(err, ErrRevoked) {
			t.Fatal(err)
		}
		if !bytes.Equal(before, snapshot(t, s)) {
			t.Fatal("revocation bypass")
		}
	})
	t.Run("key-replacement-rejects-old-authorization", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		replacement, _ := localPair(t)
		if err := s.Update(func(a ExecutionState) (ExecutionState, error) { a.TrustedPublicKey = replacement; return a, nil }); err != nil {
			t.Fatal(err)
		}
		before := snapshot(t, s)
		if err := Execute(s, intent, e, sig, artifact, apply); !errors.Is(err, ErrSignature) {
			t.Fatal(err)
		}
		if !bytes.Equal(before, snapshot(t, s)) {
			t.Fatal("old key changed state")
		}
	})
	t.Run("sequence-exhaustion", func(t *testing.T) {
		s := newDiskFixture(t, pub)
		if err := s.Update(func(a ExecutionState) (ExecutionState, error) { a.Consumed[Upgrade] = math.MaxUint64; return a, nil }); err != nil {
			t.Fatal(err)
		}
		if err := Execute(s, intent, e, sig, artifact, apply); !errors.Is(err, ErrSequenceExhausted) {
			t.Fatal(err)
		}
	})
	for _, name := range []string{"wrong-intent-chain", "wrong-intent-action", "wrong-artifact", "stale-height", "unconfigured-action", "advanced-finalized-state", "corrupt-store"} {
		t.Run(name, func(t *testing.T) {
			s := newDiskFixture(t, pub)
			i := intent
			a := bytes.Clone(artifact)
			switch name {
			case "wrong-intent-chain":
				i.ChainID = "other"
			case "wrong-intent-action":
				i.Action = Emergency
			case "wrong-artifact":
				a[0] ^= 1
			case "stale-height":
				i.FinalizedHeight = 21
			case "unconfigured-action":
				if err := s.Update(func(s ExecutionState) (ExecutionState, error) { delete(s.Consumed, Upgrade); return s, nil }); err != nil {
					t.Fatal(err)
				}
			case "advanced-finalized-state":
				if err := s.Update(func(a ExecutionState) (ExecutionState, error) { a.FinalizedHeight++; return a, nil }); err != nil {
					t.Fatal(err)
				}
			case "corrupt-store":
				if err := os.WriteFile(s.path, []byte("invalid fixture JSON"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			before := snapshot(t, s)
			called := false
			err := Execute(s, i, e, sig, a, func(a, b []byte) ([]byte, error) { called = true; return apply(a, b) })
			if err == nil || called || !bytes.Equal(before, snapshot(t, s)) {
				t.Fatal("invalid execution reached action or changed state")
			}
		})
	}
}
