package types

import (
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/slhdsa192f"
)

const (
	ABCIPubKeyTypeEd25519      = "ed25519"
	ABCIPubKeyTypeSecp256k1    = "secp256k1"
	ABCIPubKeyTypeBls12381     = "bls12381"
	ABCIPubKeyTypeMlDsa65      = mldsa65.KeyType
	ABCIPubKeyTypeSecp256k1Eth = "secp256k1eth"
	ABCIPubKeyTypeSlhDsa192f   = slhdsa192f.KeyType
)

var ABCIPubKeyTypesToNames = map[string]string{
	ABCIPubKeyTypeMlDsa65:    mldsa65.PubKeyName,
	ABCIPubKeyTypeSlhDsa192f: slhdsa192f.PubKeyName,
}
