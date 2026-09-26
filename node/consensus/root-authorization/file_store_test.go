//go:build darwin || linux

package rootauthorization

import (
	"bytes"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"

	"golang.org/x/sys/unix"
)

const fixtureStoreLimit int64 = 1 << 20

func fileState() ExecutionState {
	return ExecutionState{ChainID: "local-qualification", FinalizedHeight: 15, TrustedPublicKey: make([]byte, PublicKeySize), Consumed: map[Action]uint64{Upgrade: 6}, Application: []byte("before")}
}
func privateDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	return dir
}
func newFileStore(t *testing.T, state ExecutionState) (*FileStore, string) {
	t.Helper()
	dir := privateDir(t)
	s, err := CreateFileStore(dir, fixtureStoreLimit, state)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.Close() })
	return s, dir
}
func appendChange(s ExecutionState) (ExecutionState, error) {
	s.Application = []byte("after")
	s.Consumed[Upgrade]++
	return s, nil
}
func assertFileState(t *testing.T, s *FileStore, application string, sequence uint64) {
	t.Helper()
	v, err := s.Read()
	if err != nil || string(v.Application) != application || v.Consumed[Upgrade] != sequence {
		t.Fatalf("state=%+v error=%v", v, err)
	}
}

func TestFileStoreDurability(t *testing.T) {
	t.Run("create-update-close-reopen", func(t *testing.T) {
		s, dir := newFileStore(t, fileState())
		if err := s.Update(appendChange); err != nil {
			t.Fatal(err)
		}
		s.Close()
		r, err := OpenFileStore(dir, fixtureStoreLimit)
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		assertFileState(t, r, "after", 7)
	})
	t.Run("never-reinitialize", func(t *testing.T) {
		_, dir := newFileStore(t, fileState())
		if s, err := CreateFileStore(dir, fixtureStoreLimit, fileState()); s != nil || !errors.Is(err, ErrStoreExists) {
			t.Fatalf("%v", err)
		}
	})
	for _, stage := range []string{"before-write", "before-file-sync", "before-rename"} {
		t.Run(stage, func(t *testing.T) {
			s, dir := newFileStore(t, fileState())
			s.fault = func(v string) error {
				if v == stage {
					return errFixtureWrite
				}
				return nil
			}
			if err := s.Update(appendChange); !errors.Is(err, errFixtureWrite) {
				t.Fatal(err)
			}
			s.Close()
			r, err := OpenFileStore(dir, fixtureStoreLimit)
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			assertFileState(t, r, "before", 6)
			left, err := filepath.Glob(filepath.Join(dir, ".root-state-*"))
			if err != nil || len(left) != 0 {
				t.Fatal("temporary state remains")
			}
		})
	}
	for _, stage := range []string{"after-rename", "after-directory-sync"} {
		t.Run(stage, func(t *testing.T) {
			s, dir := newFileStore(t, fileState())
			s.fault = func(v string) error {
				if v == stage {
					return errFixtureWrite
				}
				return nil
			}
			if err := s.Update(appendChange); !errors.Is(err, ErrCommitUncertain) {
				t.Fatal(err)
			}
			if _, err := s.Read(); !errors.Is(err, ErrCommitUncertain) {
				t.Fatal(err)
			}
			if err := s.Update(appendChange); !errors.Is(err, ErrCommitUncertain) {
				t.Fatal(err)
			}
			s.Close()
			r, err := OpenFileStore(dir, fixtureStoreLimit)
			if err != nil {
				t.Fatal(err)
			}
			defer r.Close()
			assertFileState(t, r, "after", 7)
		})
	}
	t.Run("callback-error-detached-state", func(t *testing.T) {
		s, _ := newFileStore(t, fileState())
		err := s.Update(func(v ExecutionState) (ExecutionState, error) {
			v.Application[0] = 'X'
			v.Consumed[Upgrade] = 100
			v.TrustedPublicKey[0] = 1
			return v, errFixtureWrite
		})
		if !errors.Is(err, errFixtureWrite) {
			t.Fatal(err)
		}
		assertFileState(t, s, "before", 6)
	})
	t.Run("detached-read", func(t *testing.T) {
		s, _ := newFileStore(t, fileState())
		v, err := s.Read()
		if err != nil {
			t.Fatal(err)
		}
		v.Application[0] = 'X'
		v.Consumed[Upgrade] = 100
		assertFileState(t, s, "before", 6)
	})
	t.Run("closed", func(t *testing.T) {
		s, _ := newFileStore(t, fileState())
		s.Close()
		if _, err := s.Read(); !errors.Is(err, ErrStoreClosed) {
			t.Fatal(err)
		}
	})
}

func TestFileStoreStateGuards(t *testing.T) {
	mutations := map[string]func(*ExecutionState){
		"sequence-rollback": func(s *ExecutionState) { s.Consumed[Upgrade]-- },
		"height-rollback":   func(s *ExecutionState) { s.FinalizedHeight-- },
		"chain-change":      func(s *ExecutionState) { s.ChainID = "other" },
		"key-change":        func(s *ExecutionState) { s.TrustedPublicKey[0] = 1 },
		"action-removal":    func(s *ExecutionState) { delete(s.Consumed, Upgrade) },
		"action-addition":   func(s *ExecutionState) { s.Consumed[Genesis] = 0 },
		"oversized-state":   func(s *ExecutionState) { s.Application = make([]byte, fixtureStoreLimit) },
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			s, _ := newFileStore(t, fileState())
			if err := s.Update(func(v ExecutionState) (ExecutionState, error) { mutate(&v); return v, nil }); err == nil {
				t.Fatal("invalid transition accepted")
			}
			assertFileState(t, s, "before", 6)
		})
	}
	t.Run("revocation-one-way", func(t *testing.T) {
		s, _ := newFileStore(t, fileState())
		if err := s.Update(func(v ExecutionState) (ExecutionState, error) { v.Revoked = true; return v, nil }); err != nil {
			t.Fatal(err)
		}
		if err := s.Update(func(v ExecutionState) (ExecutionState, error) { v.Revoked = false; return v, nil }); !errors.Is(err, ErrExecutionState) {
			t.Fatal(err)
		}
	})
}

func TestFileStoreInputBoundaries(t *testing.T) {
	for _, name := range []string{"corrupt-checksum", "unknown-field", "trailing-data", "state-symlink", "state-fifo", "state-hardlink", "public-state-mode", "lock-symlink", "public-directory"} {
		t.Run(name, func(t *testing.T) {
			s, dir := newFileStore(t, fileState())
			s.Close()
			path := filepath.Join(dir, "state.json")
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			switch name {
			case "corrupt-checksum":
				raw = bytes.Replace(raw, []byte("YmVmb3Jl"), []byte("YmVmb3J6"), 1)
				err = os.WriteFile(path, raw, 0600)
			case "unknown-field":
				raw = append([]byte("{\"Extra\":1,"), raw[1:]...)
				err = os.WriteFile(path, raw, 0600)
			case "trailing-data":
				err = os.WriteFile(path, append(raw, '\n'), 0600)
			case "state-symlink":
				target := filepath.Join(dir, "actual.json")
				err = os.Rename(path, target)
				if err == nil {
					err = os.Symlink(target, path)
				}
			case "state-fifo":
				err = os.Remove(path)
				if err == nil {
					err = unix.Mkfifo(path, 0600)
				}
			case "state-hardlink":
				err = os.Link(path, filepath.Join(dir, "alias.json"))
			case "public-state-mode":
				err = os.Chmod(path, 0644)
			case "lock-symlink":
				p := filepath.Join(dir, "state.lock")
				err = os.Remove(p)
				if err == nil {
					err = os.Symlink(path, p)
				}
			case "public-directory":
				err = os.Chmod(dir, 0755)
			}
			if err != nil {
				t.Fatal(err)
			}
			if opened, err := OpenFileStore(dir, fixtureStoreLimit); err == nil {
				opened.Close()
				t.Fatal("invalid store accepted")
			}
		})
	}
	t.Run("explicit-storage-limit", func(t *testing.T) {
		dir := privateDir(t)
		for _, limit := range []int64{0, -1, 1} {
			if s, err := CreateFileStore(dir, limit, fileState()); err == nil {
				s.Close()
				t.Fatal("invalid size accepted")
			}
		}
	})
}

func TestFileStoreProcessLock(t *testing.T) {
	if directory := os.Getenv("DYT_ROOT_STORE_LOCK_CHILD"); directory != "" {
		s, err := OpenFileStore(directory, fixtureStoreLimit)
		if s != nil {
			s.Close()
		}
		if !errors.Is(err, ErrStoreBusy) {
			t.Fatalf("child expected locked store: %v", err)
		}
		return
	}
	s, dir := newFileStore(t, fileState())
	if err := s.Update(func(v ExecutionState) (ExecutionState, error) {
		cmd := exec.Command(os.Args[0], "-test.run=^TestFileStoreProcessLock$", "-test.count=1")
		cmd.Env = append(os.Environ(), "DYT_ROOT_STORE_LOCK_CHILD="+dir)
		if output, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("child lock check: %v %s", err, output)
		}
		return appendChange(v)
	}); err != nil {
		t.Fatal(err)
	}
	assertFileState(t, s, "after", 7)
}

func TestFileStoreAuthorizedExecution(t *testing.T) {
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	signature, err := SignForPolicy(e, priv, p)
	if err != nil {
		t.Fatal(err)
	}
	initial := fileState()
	initial.TrustedPublicKey = pub
	intent := ExecutionIntent{ChainID: p.ChainID, Action: p.Action, ArtifactDigest: p.ExpectedArtifactDigest, FinalizedHeight: p.CurrentHeight}
	artifact := []byte("local fixture only")
	transition := func(a, b []byte) ([]byte, error) {
		if string(a) != "before" || !bytes.Equal(b, artifact) {
			return nil, ErrPolicy
		}
		return []byte("after"), nil
	}
	t.Run("atomic-signed-state-restart-replay", func(t *testing.T) {
		s, dir := newFileStore(t, initial)
		if err := Execute(s, intent, e, signature, artifact, transition); err != nil {
			t.Fatal(err)
		}
		s.Close()
		r, err := OpenFileStore(dir, fixtureStoreLimit)
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		assertFileState(t, r, "after", 7)
		if err := Execute(r, intent, e, signature, artifact, transition); !errors.Is(err, ErrPolicy) {
			t.Fatal(err)
		}
	})
	t.Run("two-store-concurrent-writers", func(t *testing.T) {
		s, dir := newFileStore(t, initial)
		other, err := OpenFileStore(dir, fixtureStoreLimit)
		if err != nil {
			t.Fatal(err)
		}
		defer other.Close()
		var success atomic.Int32
		var wg sync.WaitGroup
		for i := 0; i < 8; i++ {
			target := s
			if i%2 == 1 {
				target = other
			}
			wg.Add(1)
			go func(s *FileStore) {
				defer wg.Done()
				err := Execute(s, intent, e, signature, artifact, transition)
				if err == nil {
					success.Add(1)
				} else if !errors.Is(err, ErrPolicy) && !errors.Is(err, ErrStoreBusy) {
					t.Errorf("unexpected error %v", err)
				}
			}(target)
		}
		wg.Wait()
		if success.Load() != 1 {
			t.Fatalf("successful writers %d", success.Load())
		}
		assertFileState(t, s, "after", 7)
	})
	t.Run("uncertain-commit-replay-consistency", func(t *testing.T) {
		s, dir := newFileStore(t, initial)
		s.fault = func(stage string) error {
			if stage == "after-directory-sync" {
				return errFixtureWrite
			}
			return nil
		}
		if err := Execute(s, intent, e, signature, artifact, transition); !errors.Is(err, ErrCommitUncertain) {
			t.Fatal(err)
		}
		s.Close()
		r, err := OpenFileStore(dir, fixtureStoreLimit)
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		if err := Execute(r, intent, e, signature, artifact, transition); !errors.Is(err, ErrPolicy) {
			t.Fatal(err)
		}
		assertFileState(t, r, "after", 7)
	})
}
