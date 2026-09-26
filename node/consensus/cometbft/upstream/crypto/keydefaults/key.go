package keydefaults

import "github.com/cometbft/cometbft/crypto"

func MustGenerate() crypto.PrivKey {
	key, err := Generate()
	if err != nil {
		panic(err)
	}
	return key
}
