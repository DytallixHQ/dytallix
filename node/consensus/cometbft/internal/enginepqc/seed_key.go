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
	path := filepath.Join(home, "config", SeedFileName)
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, nil, err
	}
	file := os.NewFile(uintptr(fd), path)
	defer file.Close()
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return nil, nil, err
	}
	mode := stat.Mode & 0o777
	if stat.Mode&unix.S_IFMT != unix.S_IFREG || (mode != 0o400 && mode != 0o600) || stat.Size != mldsa65.SeedSize || stat.Nlink != 1 || stat.Uid != uint32(os.Geteuid()) {
		return nil, nil, fmt.Errorf("peer seed requires one owner-only regular file of %d bytes", mldsa65.SeedSize)
	}
	seed := make([]byte, mldsa65.SeedSize)
	defer clear(seed)
	if _, err := io.ReadFull(file, seed); err != nil {
		return nil, nil, err
	}
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
