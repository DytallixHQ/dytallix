package pqcp2p

import (
	"bytes"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

// ImportSeedIdentity derives a peer identity from an exact ML-DSA-65 seed.
// It requires the full expected public key so a wrong seed cannot select a
// different peer identity. The caller owns seed and must protect its storage.
// Go does not guarantee erasure of derived private keys from memory.
// This function does not enable a production transport or define key custody.
func ImportSeedIdentity(seed, expectedPublic []byte) (*Identity, error) {
	if len(seed) != mldsa65.SeedSize || len(expectedPublic) != mldsa65.PublicKeySize {
		return nil, ErrRejected
	}
	var fixed [mldsa65.SeedSize]byte
	copy(fixed[:], seed)
	defer clear(fixed[:])
	public, private := mldsa65.NewKeyFromSeed(&fixed)
	if !bytes.Equal(public.Bytes(), expectedPublic) {
		return nil, ErrRejected
	}
	return &Identity{private: private, public: public}, nil
}
