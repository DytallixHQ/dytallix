package main

import (
	"encoding/base64"
	"encoding/json"
	"math"
	"strings"
	"testing"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/types"
)

// The largest InitChain line: an 8 MiB application genesis in base64 with
// 64 validators (production activation v1, A6). It must fit one line, as
// child.call encodes it.
func TestInitChainCarriesTheLargestApplicationGenesis(t *testing.T) {
	validators := make([]validator, maxValidators)
	for i := range validators {
		validators[i] = validator{mldsa65.KeyType, base64.StdEncoding.EncodeToString(make([]byte, mldsa65.PubKeySize)), types.MaxTotalVotingPower / maxValidators}
	}
	payload := map[string]any{"chain_id": strings.Repeat("c", 64), "initial_height": int64(1),
		"app_state_bytes": base64.StdEncoding.EncodeToString(make([]byte, 8<<20)), "validators": validators,
		"evidence_max_age_blocks": int64(math.MaxInt64), "evidence_max_age_seconds": int64(math.MaxInt64), "evidence_max_age_nanos": int32(999_999_999)}
	request, err := json.Marshal(struct {
		Method  string `json:"method"`
		Payload any    `json:"payload"`
	}{"init_chain", payload})
	if err != nil {
		t.Fatal(err)
	}
	if len(request)+1 > maxJSONBytes {
		t.Fatalf("an InitChain line of %d bytes exceeds the %d-byte bound", len(request)+1, maxJSONBytes)
	}
	// The former 8 MiB line could not carry it.
	if len(request)+1 <= 8<<20 {
		t.Fatalf("the largest InitChain line is only %d bytes", len(request)+1)
	}
}
