//go:build dytallix_pqc_only

package types

import "github.com/cometbft/cometbft/crypto/mldsa65"

var MaxSignatureSize = mldsa65.SignatureSize

// Signable is an interface for all signable things.
// It typically removes signatures before serializing.
// SignBytes returns the bytes to be signed
// NOTE: chainIDs are part of the SignBytes but not
// necessarily the object themselves.
// NOTE: Expected to panic if there is an error marshaling.
type Signable interface {
	SignBytes(chainID string) []byte
}
