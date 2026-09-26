//go:build dytallix_pqc_only

package types

import cryptoenc "github.com/cometbft/cometbft/crypto/encoding"

// UpdateValidator rejects all key types except the profile's ML-DSA-65 type.
func UpdateValidator(pk []byte, power int64, keyType string) ValidatorUpdate {
	key, err := cryptoenc.PubKeyFromTypeAndBytes(keyType, pk)
	if err != nil {
		panic(err)
	}
	encoded, err := cryptoenc.PubKeyToProto(key)
	if err != nil {
		panic(err)
	}
	return ValidatorUpdate{PubKey: encoded, Power: power}
}

// Ed25519ValidatorUpdate fails closed in this profile.
func Ed25519ValidatorUpdate([]byte, int64) ValidatorUpdate {
	panic("ed25519 excluded by dytallix_pqc_only")
}
