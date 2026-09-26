//go:build !dytallix_pqc_only

package keydefaults

import (
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/ed25519"
)

const KeyType = ed25519.KeyType

func Generate() (crypto.PrivKey, error) { return ed25519.GenPrivKey(), nil }
