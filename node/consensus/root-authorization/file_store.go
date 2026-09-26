//go:build darwin || linux

package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"sync"

	"golang.org/x/sys/unix"
)

var (
	ErrStoreBusy       = errors.New("root store is locked by another writer")
	ErrStoreClosed     = errors.New("root store is closed")
	ErrCommitUncertain = errors.New("root commit may have persisted; reopen and inspect durable state")
	ErrStoreExists     = errors.New("root store already exists")
)

// FileStore owns one complete state document. It is not an adapter to the chain's
// RocksDB state. Never use it as a second replay ledger for a Rust chain action.
// Use a local filesystem with working flock, atomic same-directory rename and
// fsync. Network filesystems and storage hardware failure are not qualified.
// The directory must exist, belong to this effective user, and exclude all group
// and other permissions. Operators must not replace/remove the directory or lock
// file while open. Privileged writers that bypass this API remain trusted.
type FileStore struct {
	mu        sync.Mutex
	dir       *os.File
	maxBytes  int64
	uncertain bool
	// fault is package-private fault injection for deterministic storage tests.
	fault func(string) error
}

type storedState struct {
	Version  uint16
	State    ExecutionState
	Checksum [sha512.Size]byte
}

// OpenFileStore opens an existing store. maxBytes is an explicit caller storage
// bound, not an approved production protocol parameter. No default is inferred.
func OpenFileStore(directory string, maxBytes int64) (*FileStore, error) {
	s, err := openDirectory(directory, maxBytes)
	if err != nil {
		return nil, err
	}
	err = s.locked(func() error {
		_, err := s.read()
		if err != nil {
			return err
		}
		return s.dir.Sync()
	})
	if err != nil {
		s.Close()
		return nil, err
	}
	return s, nil
}

// CreateFileStore creates one new state without replacing an existing file.
// Initial authority and permitted actions must come from trusted configuration.
func CreateFileStore(directory string, maxBytes int64, initial ExecutionState) (*FileStore, error) {
	if err := validateStoredState(initial); err != nil {
		return nil, err
	}
	s, err := openDirectory(directory, maxBytes)
	if err != nil {
		return nil, err
	}
	err = s.locked(func() error {
		var stat unix.Stat_t
		err := unix.Fstatat(int(s.dir.Fd()), "state.json", &stat, unix.AT_SYMLINK_NOFOLLOW)
		if err == nil {
			return ErrStoreExists
		}
		if !errors.Is(err, unix.ENOENT) {
			return err
		}
		return s.persist(initial)
	})
	if err != nil {
		s.Close()
		return nil, err
	}
	return s, nil
}

func openDirectory(path string, maxBytes int64) (*FileStore, error) {
	if maxBytes <= 0 || maxBytes == int64(^uint64(0)>>1) {
		return nil, ErrExecutionState
	}
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	dir := os.NewFile(uintptr(fd), path)
	var st unix.Stat_t
	if err = unix.Fstat(fd, &st); err != nil {
		dir.Close()
		return nil, err
	}
	if st.Mode&077 != 0 || st.Uid != uint32(os.Geteuid()) {
		dir.Close()
		return nil, ErrExecutionState
	}
	return &FileStore{dir: dir, maxBytes: maxBytes}, nil
}

func (s *FileStore) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.dir == nil {
		return nil
	}
	err := s.dir.Close()
	s.dir = nil
	return err
}

func privateRegular(fd int) error {
	var st unix.Stat_t
	if err := unix.Fstat(fd, &st); err != nil {
		return err
	}
	if st.Mode&unix.S_IFMT != unix.S_IFREG || st.Mode&077 != 0 || st.Uid != uint32(os.Geteuid()) || st.Nlink != 1 {
		return ErrExecutionState
	}
	return nil
}

func (s *FileStore) locked(fn func() error) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.dir == nil {
		return ErrStoreClosed
	}
	if s.uncertain {
		return ErrCommitUncertain
	}
	fd, err := unix.Openat(int(s.dir.Fd()), "state.lock", unix.O_RDWR|unix.O_CREAT|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
	if err != nil {
		return err
	}
	defer unix.Close(fd)
	if err = privateRegular(fd); err != nil {
		return err
	}
	if err = unix.Flock(fd, unix.LOCK_EX|unix.LOCK_NB); err != nil {
		if errors.Is(err, unix.EWOULDBLOCK) || errors.Is(err, unix.EAGAIN) {
			return ErrStoreBusy
		}
		return err
	}
	defer unix.Flock(fd, unix.LOCK_UN)
	return fn()
}

func validateStoredState(state ExecutionState) error {
	if !validChainID(state.ChainID) || len(state.Consumed) == 0 {
		return ErrExecutionState
	}
	if err := ValidatePublicKey(state.TrustedPublicKey); err != nil {
		return err
	}
	for action := range state.Consumed {
		if !validAction(action) {
			return ErrExecutionState
		}
	}
	return nil
}

func encodeState(state ExecutionState) ([]byte, error) {
	if err := validateStoredState(state); err != nil {
		return nil, err
	}
	payload, err := json.Marshal(state)
	if err != nil {
		return nil, err
	}
	return json.Marshal(storedState{Version: 1, State: state, Checksum: sha512.Sum512(payload)})
}

func (s *FileStore) read() (ExecutionState, error) {
	var empty ExecutionState
	fd, err := unix.Openat(int(s.dir.Fd()), "state.json", unix.O_RDONLY|unix.O_NONBLOCK|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return empty, err
	}
	f := os.NewFile(uintptr(fd), "state.json")
	defer f.Close()
	if err = privateRegular(fd); err != nil {
		return empty, err
	}
	info, err := f.Stat()
	if err != nil {
		return empty, err
	}
	if info.Size() <= 0 || info.Size() > s.maxBytes {
		return empty, ErrExecutionState
	}
	raw, err := io.ReadAll(io.LimitReader(f, s.maxBytes+1))
	if err != nil {
		return empty, err
	}
	if int64(len(raw)) > s.maxBytes {
		return empty, ErrExecutionState
	}
	var record storedState
	if err = json.Unmarshal(raw, &record); err != nil {
		return empty, err
	}
	if record.Version != 1 {
		return empty, ErrExecutionState
	}
	canonical, err := encodeState(record.State)
	if err != nil {
		return empty, err
	}
	// Exact canonical equality also rejects duplicate/unknown fields, trailing
	// whitespace, altered checksums and ambiguous JSON representations.
	if !bytes.Equal(canonical, raw) {
		return empty, ErrExecutionState
	}
	return record.State, nil
}

// Read returns a detached state snapshot under the process lock. An uncertain
// writer must close and reopen before reading; this forces explicit recovery.
func (s *FileStore) Read() (ExecutionState, error) {
	var state ExecutionState
	err := s.locked(func() error { var err error; state, err = s.read(); return err })
	return state, err
}

// Update commits a complete local state under both the in-process and OS lock.
// It forbids changes of chain, root key, permitted actions, backwards finalized
// height or backwards sequences. Revocation is one-way. Key replacement and
// reactivation require a separately reviewed API and are not implemented here.
func (s *FileStore) Update(update func(ExecutionState) (ExecutionState, error)) error {
	if update == nil {
		return ErrExecutionState
	}
	return s.locked(func() error {
		previous, err := s.read()
		if err != nil {
			return err
		}
		// Save immutable bytes before handing detached input to the callback.
		before, err := json.Marshal(previous)
		if err != nil {
			return err
		}
		next, err := update(previous)
		if err != nil {
			return err
		}
		var old ExecutionState
		if err = json.Unmarshal(before, &old); err != nil {
			return err
		}
		if err = validateStoredState(next); err != nil {
			return err
		}
		if next.ChainID != old.ChainID || !bytes.Equal(next.TrustedPublicKey, old.TrustedPublicKey) || next.FinalizedHeight < old.FinalizedHeight || old.Revoked && !next.Revoked || len(next.Consumed) != len(old.Consumed) {
			return ErrExecutionState
		}
		for action, sequence := range old.Consumed {
			value, ok := next.Consumed[action]
			if !ok || value < sequence {
				return ErrExecutionState
			}
		}
		return s.persist(next)
	})
}

func (s *FileStore) checkpoint(stage string) error {
	if s.fault != nil {
		return s.fault(stage)
	}
	return nil
}

func (s *FileStore) persist(next ExecutionState) error {
	raw, err := encodeState(next)
	if err != nil {
		return err
	}
	if int64(len(raw)) > s.maxBytes {
		return ErrExecutionState
	}
	if err = s.checkpoint("before-write"); err != nil {
		return err
	}
	var nonce [16]byte
	if _, err = rand.Read(nonce[:]); err != nil {
		return err
	}
	name := ".root-state-" + hex.EncodeToString(nonce[:])
	fd, err := unix.Openat(int(s.dir.Fd()), name, unix.O_WRONLY|unix.O_CREAT|unix.O_EXCL|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0600)
	if err != nil {
		return err
	}
	defer unix.Unlinkat(int(s.dir.Fd()), name, 0)
	f := os.NewFile(uintptr(fd), name)
	if _, err = f.Write(raw); err != nil {
		f.Close()
		return err
	}
	if err = s.checkpoint("before-file-sync"); err != nil {
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
	if err = s.checkpoint("before-rename"); err != nil {
		return err
	}
	if err = unix.Renameat(int(s.dir.Fd()), name, int(s.dir.Fd()), "state.json"); err != nil {
		return err
	}
	// After rename, an error cannot establish that the old state remains durable.
	if err = s.checkpoint("after-rename"); err == nil {
		err = s.dir.Sync()
	}
	if err != nil {
		s.uncertain = true
		return fmt.Errorf("%w: %v", ErrCommitUncertain, err)
	}
	if err = s.checkpoint("after-directory-sync"); err != nil {
		s.uncertain = true
		return fmt.Errorf("%w: %v", ErrCommitUncertain, err)
	}
	return nil
}
