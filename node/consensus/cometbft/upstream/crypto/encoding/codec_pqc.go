//go:build dytallix_pqc_only

package encoding

import (
	"fmt"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/libs/json"
	pc "github.com/cometbft/cometbft/proto/tendermint/crypto"
)

// ErrUnsupportedKey describes an error resulting from the use of an
// unsupported key in [PubKeyToProto] or [PubKeyFromProto].
type ErrUnsupportedKey struct {
	Key any
}

func (e ErrUnsupportedKey) Error() string {
	return fmt.Sprintf("encoding: unsupported key %v", e.Key)
}

// ErrInvalidKeyLen describes an error resulting from the use of a key with
// an invalid length in [PubKeyFromProto].
type ErrInvalidKeyLen struct {
	Key       any
	Got, Want int
}

func (e ErrInvalidKeyLen) Error() string {
	return fmt.Sprintf("encoding: invalid key length for %v, got %d, want %d", e.Key, e.Got, e.Want)
}

func init() {
	json.RegisterType((*pc.PublicKey)(nil), "tendermint.crypto.PublicKey")
	json.RegisterType((*pc.PublicKey_Mldsa65)(nil), "tendermint.crypto.PublicKey_Mldsa65")
}
func PubKeyToProto(k crypto.PubKey) (pc.PublicKey, error) {
	if k, ok := k.(mldsa65.PubKey); ok {
		return pc.PublicKey{Sum: &pc.PublicKey_Mldsa65{Mldsa65: k.Bytes()}}, nil
	}
	return pc.PublicKey{}, ErrUnsupportedKey{Key: k}
}
func PubKeyFromProto(k pc.PublicKey) (crypto.PubKey, error) {
	if key, ok := k.Sum.(*pc.PublicKey_Mldsa65); ok && key != nil {
		return PubKeyFromTypeAndBytes(mldsa65.KeyType, key.Mldsa65)
	}
	return nil, ErrUnsupportedKey{Key: k.Sum}
}
func PubKeyFromTypeAndBytes(kind string, b []byte) (crypto.PubKey, error) {
	if kind != mldsa65.KeyType {
		return nil, ErrUnsupportedKey{Key: kind}
	}
	if len(b) != mldsa65.PubKeySize {
		return nil, ErrInvalidKeyLen{Key: kind, Got: len(b), Want: mldsa65.PubKeySize}
	}
	return mldsa65.NewPubKeyFromBytes(b)
}
