package types

import (
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/slhdsa192f"
)

// MaxSignatureSize is the largest signature of any registered validator key type.
var MaxSignatureSize = max(mldsa65.SignatureSize, slhdsa192f.SignatureSize)

// Signable is an interface for all signable things.
// It typically removes signatures before serializing.
// SignBytes returns the bytes to be signed
// NOTE: chainIDs are part of the SignBytes but not
// necessarily the object themselves.
// NOTE: Expected to panic if there is an error marshaling.
type Signable interface {
	SignBytes(chainID string) []byte
}
