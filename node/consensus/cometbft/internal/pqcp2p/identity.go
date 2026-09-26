package pqcp2p

import (
	"bytes"
	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

// ImportIdentity accepts a packed CIRCL ML-DSA-65 private key, not a seed.
// It checks the encoding and signing capability of trusted local key files.
// It does not certify complete packed-key consistency: a small inconsistent t0
// change can still produce a valid signature. Production key import requires a
// reviewed full-consistency API or a seed-derived identity design.
// The caller retains ownership of its input. Go does not guarantee key erasure.
func ImportIdentity(raw []byte) (identity *Identity, err error) {
	defer func() {
		if recover() != nil {
			identity = nil
			err = ErrRejected
		}
	}()
	if len(raw) != mldsa65.PrivateKeySize {
		return nil, ErrRejected
	}
	// rho(32), K(32), tr(64), then eleven eta=4 polynomials of 128 bytes.
	// Each nibble encodes an integer in [0,8]. Unpack alone does not check this.
	for _, b := range raw[128:1536] {
		if b&15 > 8 || b>>4 > 8 {
			return nil, ErrRejected
		}
	}
	var packed [mldsa65.PrivateKeySize]byte
	copy(packed[:], raw)
	defer clear(packed[:])
	var private mldsa65.PrivateKey
	if private.UnmarshalBinary(packed[:]) != nil {
		return nil, ErrRejected
	}
	canonical := private.Bytes()
	defer clear(canonical)
	if !bytes.Equal(canonical, packed[:]) {
		return nil, ErrRejected
	}
	derived, ok := private.Public().(*mldsa65.PublicKey)
	if !ok {
		return nil, ErrRejected
	}
	// Reparse: Public() shares the private key's cached tr. A fresh public key
	// must recompute that hash to detect inconsistent imported fields.
	var public mldsa65.PublicKey
	if public.UnmarshalBinary(derived.Bytes()) != nil {
		return nil, ErrRejected
	}
	sig := make([]byte, mldsa65.SignatureSize)
	defer clear(sig)
	message := []byte("dytallix-pqcp2p-identity-import-v1")
	context := []byte(Suite + "/identity-import")
	if mldsa65.SignTo(&private, message, context, false, sig) != nil || !mldsa65.Verify(&public, message, context, sig) {
		return nil, ErrRejected
	}
	return &Identity{private: &private, public: &public}, nil
}
