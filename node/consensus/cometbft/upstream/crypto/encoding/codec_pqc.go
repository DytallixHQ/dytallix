package encoding

import (
	"fmt"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/slhdsa192f"
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
	json.RegisterType((*pc.PublicKey_SlhdsaShake_192F)(nil), "tendermint.crypto.PublicKey_SlhdsaShake_192F")
}
func PubKeyToProto(k crypto.PubKey) (pc.PublicKey, error) {
	switch k := k.(type) {
	case mldsa65.PubKey:
		return pc.PublicKey{Sum: &pc.PublicKey_Mldsa65{Mldsa65: k.Bytes()}}, nil
	case slhdsa192f.PubKey:
		return pc.PublicKey{Sum: &pc.PublicKey_SlhdsaShake_192F{SlhdsaShake_192F: k.Bytes()}}, nil
	}
	return pc.PublicKey{}, ErrUnsupportedKey{Key: k}
}
func PubKeyFromProto(k pc.PublicKey) (crypto.PubKey, error) {
	switch key := k.Sum.(type) {
	case *pc.PublicKey_Mldsa65:
		if key != nil {
			return PubKeyFromTypeAndBytes(mldsa65.KeyType, key.Mldsa65)
		}
	case *pc.PublicKey_SlhdsaShake_192F:
		if key != nil {
			return PubKeyFromTypeAndBytes(slhdsa192f.KeyType, key.SlhdsaShake_192F)
		}
	}
	return nil, ErrUnsupportedKey{Key: k.Sum}
}

func PubKeyFromTypeAndBytes(kind string, b []byte) (crypto.PubKey, error) {
	switch kind {
	case mldsa65.KeyType:
		if len(b) != mldsa65.PubKeySize {
			return nil, ErrInvalidKeyLen{Key: kind, Got: len(b), Want: mldsa65.PubKeySize}
		}
		return mldsa65.NewPubKeyFromBytes(b)
	case slhdsa192f.KeyType:
		if len(b) != slhdsa192f.PubKeySize {
			return nil, ErrInvalidKeyLen{Key: kind, Got: len(b), Want: slhdsa192f.PubKeySize}
		}
		return slhdsa192f.NewPubKeyFromBytes(b)
	}
	return nil, ErrUnsupportedKey{Key: kind}
}
