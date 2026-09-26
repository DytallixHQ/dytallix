//go:build !dytallix_pqc_only

package types

import (
	"github.com/cometbft/cometbft/crypto/bls12381"
	"github.com/cometbft/cometbft/crypto/ed25519"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/secp256k1"
	"github.com/cometbft/cometbft/crypto/secp256k1eth"
)

const (
	ABCIPubKeyTypeEd25519      = ed25519.KeyType
	ABCIPubKeyTypeSecp256k1    = secp256k1.KeyType
	ABCIPubKeyTypeBls12381     = bls12381.KeyType
	ABCIPubKeyTypeMlDsa65      = mldsa65.KeyType
	ABCIPubKeyTypeSecp256k1Eth = secp256k1eth.KeyType
)

var ABCIPubKeyTypesToNames = map[string]string{
	ABCIPubKeyTypeEd25519:      ed25519.PubKeyName,
	ABCIPubKeyTypeSecp256k1:    secp256k1.PubKeyName,
	ABCIPubKeyTypeMlDsa65:      mldsa65.PubKeyName,
	ABCIPubKeyTypeSecp256k1Eth: secp256k1eth.PubKeyName,
}

func init() {
	if bls12381.Enabled {
		ABCIPubKeyTypesToNames[ABCIPubKeyTypeBls12381] = bls12381.PubKeyName
	}
}
