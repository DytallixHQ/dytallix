// Package slhdsa192f implements FIPS 205 SLH-DSA-SHAKE-192f as a CometBFT
// validator key type. It is the crypto-agility spike's hash-based fallback
// for validator votes: category 3 like ML-DSA-65, and the fast ("f") set
// because the small ("s") sets take over a second per signature.
//
// Signing is deterministic (no added randomness) over an empty context, the
// pure mode used by the mldsa65 key type. Nothing selects this type yet: the
// proto PublicKey oneof has no field for it, so it cannot cross ABCI.
package slhdsa192f

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"

	"github.com/cloudflare/circl/sign/slhdsa"

	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/tmhash"
	cmtjson "github.com/cometbft/cometbft/libs/json"
)

const (
	// PrivKeySize is the size, in bytes, of a packed private key.
	PrivKeySize = 96
	// PubKeySize is the size, in bytes, of a public key.
	PubKeySize = 48
	// SignatureSize is the size, in bytes, of a signature.
	SignatureSize = 35664

	// KeyType is the string identifier for the algorithm.
	KeyType = "slh_dsa_shake_192f"
	// PrivKeyName is the amino route for the private key.
	PrivKeyName = "cometbft/PrivKeySlhDsaShake192f"
	// PubKeyName is the amino route for the public key.
	PubKeyName = "cometbft/PubKeySlhDsaShake192f"

	id = slhdsa.SHAKE_192f
)

var (
	ErrInvalidPrivKeySize = fmt.Errorf("slhdsa192f: invalid private key size, expected %d bytes", PrivKeySize)
	ErrInvalidPubKeySize  = fmt.Errorf("slhdsa192f: invalid public key size, expected %d bytes", PubKeySize)
	ErrDeserialization    = errors.New("slhdsa192f: deserialization error")
)

func init() {
	cmtjson.RegisterType(PubKey{}, PubKeyName)
	cmtjson.RegisterType(PrivKey{}, PrivKeyName)
}

var _ crypto.PrivKey = PrivKey{}

// PrivKey wraps a parsed SLH-DSA-SHAKE-192f private key.
type PrivKey struct {
	sk *slhdsa.PrivateKey
}

// GenPrivKey generates a fresh key using OS randomness.
func GenPrivKey() (PrivKey, error) {
	return genPrivKey(crypto.CReader())
}

func genPrivKey(r io.Reader) (PrivKey, error) {
	_, sk, err := slhdsa.GenerateKey(r, id)
	if err != nil {
		return PrivKey{}, err
	}
	return PrivKey{sk: &sk}, nil
}

// NewPrivKeyFromBytes validates and returns a PrivKey from the packed bytes.
func NewPrivKeyFromBytes(bz []byte) (PrivKey, error) {
	if len(bz) != PrivKeySize {
		return PrivKey{}, ErrInvalidPrivKeySize
	}
	sk := &slhdsa.PrivateKey{ID: id}
	if err := sk.UnmarshalBinary(bz); err != nil {
		return PrivKey{}, ErrDeserialization
	}
	return PrivKey{sk: sk}, nil
}

// Bytes returns the packed private key bytes.
func (privKey PrivKey) Bytes() []byte {
	if privKey.sk == nil {
		return nil
	}
	bz, err := privKey.sk.MarshalBinary()
	if err != nil {
		panic(fmt.Sprintf("slhdsa192f: marshal privkey: %v", err))
	}
	return bz
}

// PubKey returns the corresponding public key.
func (privKey PrivKey) PubKey() crypto.PubKey {
	pk := privKey.sk.PublicKey()
	return PubKey{pk: &pk}
}

// Equals returns true if the other key is the same type and the bytes match.
func (privKey PrivKey) Equals(other crypto.PrivKey) bool {
	otherKey, ok := other.(PrivKey)
	if !ok {
		return false
	}
	return bytes.Equal(privKey.Bytes(), otherKey.Bytes())
}

// Type returns the algorithm identifier.
func (PrivKey) Type() string {
	return KeyType
}

// Sign produces a deterministic signature with an empty context.
func (privKey PrivKey) Sign(msg []byte) ([]byte, error) {
	return slhdsa.SignDeterministic(privKey.sk, slhdsa.NewMessage(msg), nil)
}

// MarshalJSON marshals the private key to JSON as its packed bytes.
func (privKey PrivKey) MarshalJSON() ([]byte, error) {
	return json.Marshal(privKey.Bytes())
}

// UnmarshalJSON unmarshals the private key from JSON.
func (privKey *PrivKey) UnmarshalJSON(bz []byte) error {
	var rawBytes []byte
	if err := json.Unmarshal(bz, &rawBytes); err != nil {
		return err
	}
	pk, err := NewPrivKeyFromBytes(rawBytes)
	if err != nil {
		return err
	}
	privKey.sk = pk.sk
	return nil
}

var _ crypto.PubKey = PubKey{}

// PubKey wraps a parsed SLH-DSA-SHAKE-192f public key.
type PubKey struct {
	pk *slhdsa.PublicKey
}

// NewPubKeyFromBytes validates and returns a PubKey from the packed bytes.
func NewPubKeyFromBytes(bz []byte) (PubKey, error) {
	if len(bz) != PubKeySize {
		return PubKey{}, ErrInvalidPubKeySize
	}
	pk := &slhdsa.PublicKey{ID: id}
	if err := pk.UnmarshalBinary(bz); err != nil {
		return PubKey{}, ErrDeserialization
	}
	return PubKey{pk: pk}, nil
}

// Address is SHA256(pubkey) truncated to 20 bytes, as for mldsa65.
func (pubKey PubKey) Address() crypto.Address {
	return crypto.Address(tmhash.SumTruncated(pubKey.Bytes()))
}

// VerifySignature verifies the signature against msg with an empty context.
func (pubKey PubKey) VerifySignature(msg, sig []byte) bool {
	if len(sig) != SignatureSize || pubKey.pk == nil {
		return false
	}
	return slhdsa.Verify(pubKey.pk, slhdsa.NewMessage(msg), sig, nil)
}

// Bytes returns the packed public key bytes.
func (pubKey PubKey) Bytes() []byte {
	if pubKey.pk == nil {
		return nil
	}
	bz, err := pubKey.pk.MarshalBinary()
	if err != nil {
		panic(fmt.Sprintf("slhdsa192f: marshal pubkey: %v", err))
	}
	return bz
}

// Type returns the algorithm identifier.
func (PubKey) Type() string {
	return KeyType
}

// Equals returns true if the other key is the same type and the bytes match.
func (pubKey PubKey) Equals(other crypto.PubKey) bool {
	otherKey, ok := other.(PubKey)
	if !ok {
		return false
	}
	return bytes.Equal(pubKey.Bytes(), otherKey.Bytes())
}

// String returns a hex-formatted representation of the public key.
func (pubKey PubKey) String() string {
	return fmt.Sprintf("PubKeySlhDsaShake192f{%X}", pubKey.Bytes())
}

// MarshalJSON marshals the public key to JSON as its packed bytes.
func (pubKey PubKey) MarshalJSON() ([]byte, error) {
	return json.Marshal(pubKey.Bytes())
}

// UnmarshalJSON unmarshals the public key from JSON.
func (pubKey *PubKey) UnmarshalJSON(bz []byte) error {
	var rawBytes []byte
	if err := json.Unmarshal(bz, &rawBytes); err != nil {
		return err
	}
	pk, err := NewPubKeyFromBytes(rawBytes)
	if err != nil {
		return err
	}
	pubKey.pk = pk.pk
	return nil
}
