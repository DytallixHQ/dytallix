package keytypes

import (
	"fmt"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/slhdsa192f"
)

func GenPrivKey(kind string) (crypto.PrivKey, error) {
	switch kind {
	case mldsa65.KeyType:
		return mldsa65.GenPrivKey()
	case slhdsa192f.KeyType:
		return slhdsa192f.GenPrivKey()
	}
	return nil, fmt.Errorf("unsupported key type: %q", kind)
}
func SupportedKeyTypesStr() string {
	return fmt.Sprintf("%q, %q", mldsa65.KeyType, slhdsa192f.KeyType)
}
func ListSupportedKeyTypes() []string { return []string{mldsa65.KeyType, slhdsa192f.KeyType} }
