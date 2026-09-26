//go:build dytallix_pqc_only

package batch

import "github.com/cometbft/cometbft/crypto"

// ML-DSA verification uses the existing individual verifier.
func CreateBatchVerifier(crypto.PubKey) (crypto.BatchVerifier, bool) { return nil, false }
func SupportsBatchVerifier(crypto.PubKey) bool                       { return false }
