package enginepqc

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
	"golang.org/x/sys/unix"
)

const SeedFileName = "pqc_peer_seed.bin"

// seedNodeKey opens the experimental raw seed without following a symlink.
// The packed Comet node key must not coexist with this seed-backed profile.
func seedNodeKey(home string, expectedPublic []byte) (*p2p.NodeKey, *pqcp2p.Identity, error) {
	packedPath := filepath.Join(home, "config", "node_key.json")
	if _, err := os.Lstat(packedPath); err == nil {
		return nil, nil, errors.New("packed peer key is forbidden in seed profile")
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, nil, err
	}
	seed, err := readPeerSeed(home)
	if err != nil {
		return nil, nil, err
	}
	defer clear(seed)
	private, err := mldsa65.GenPrivKeyFromSeed(seed)
	if err != nil {
		return nil, nil, err
	}
	key := &p2p.NodeKey{PrivKey: private}
	if !bytes.Equal(key.PubKey().Bytes(), expectedPublic) {
		return nil, nil, errors.New("seed-derived peer key differs from the full public pin")
	}
	identity, err := pqcp2p.ImportSeedIdentity(seed, expectedPublic)
	if err != nil {
		return nil, nil, err
	}
	if !bytes.Equal(identity.PublicKey(), key.PubKey().Bytes()) {
		return nil, nil, errors.New("seed-derived peer identities disagree")
	}
	return key, identity, nil
}

// readPeerSeed reads the raw peer seed: one owner-only regular file of
// exactly the seed size, opened without following a symlink. The caller
// clears the returned bytes.
func readPeerSeed(home string) ([]byte, error) {
	path := filepath.Join(home, "config", SeedFileName)
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(fd), path)
	defer file.Close()
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return nil, err
	}
	mode := stat.Mode & 0o777
	if stat.Mode&unix.S_IFMT != unix.S_IFREG || (mode != 0o400 && mode != 0o600) || stat.Size != mldsa65.SeedSize || stat.Nlink != 1 || stat.Uid != uint32(os.Geteuid()) {
		return nil, fmt.Errorf("peer seed requires one owner-only regular file of %d bytes", mldsa65.SeedSize)
	}
	seed := make([]byte, mldsa65.SeedSize)
	if _, err := io.ReadFull(file, seed); err != nil {
		clear(seed)
		return nil, err
	}
	return seed, nil
}

// PeerSeedPublicKey is the ML-DSA-65 peer public key the seed at
// HOME/config/pqc_peer_seed.bin derives: the node's own transport pin and
// the pin its peers hold (production activation v1, A5).
func PeerSeedPublicKey(home string) ([]byte, error) {
	seed, err := readPeerSeed(home)
	if err != nil {
		return nil, err
	}
	defer clear(seed)
	private, err := mldsa65.GenPrivKeyFromSeed(seed)
	if err != nil {
		return nil, err
	}
	return private.PubKey().Bytes(), nil
}
