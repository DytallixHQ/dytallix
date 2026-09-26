//go:build dytallix_pqc_only

package keydefaults

import (
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
)

const KeyType = mldsa65.KeyType

func Generate() (crypto.PrivKey, error) { return mldsa65.GenPrivKey() }
