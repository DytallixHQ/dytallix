//go:build dytallix_pqc_only

package keytypes

import (
	"fmt"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
)

func GenPrivKey(kind string) (crypto.PrivKey, error) {
	if kind != mldsa65.KeyType {
		return nil, fmt.Errorf("unsupported key type: %q", kind)
	}
	return mldsa65.GenPrivKey()
}
func SupportedKeyTypesStr() string    { return fmt.Sprintf("%q", mldsa65.KeyType) }
func ListSupportedKeyTypes() []string { return []string{mldsa65.KeyType} }
