//go:build dytallix_pqc_only

package types

import "github.com/cometbft/cometbft/crypto/mldsa65"

const (
	ABCIPubKeyTypeEd25519      = "ed25519"
	ABCIPubKeyTypeSecp256k1    = "secp256k1"
	ABCIPubKeyTypeBls12381     = "bls12381"
	ABCIPubKeyTypeMlDsa65      = mldsa65.KeyType
	ABCIPubKeyTypeSecp256k1Eth = "secp256k1eth"
)

var ABCIPubKeyTypesToNames = map[string]string{ABCIPubKeyTypeMlDsa65: mldsa65.PubKeyName}
