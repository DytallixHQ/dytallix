package main

import (
	"bytes"
	"context"
	"encoding/json"
	"testing"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/types"
)

// Neutral metadata tests the application boundary only. No vote is constructed
// or signed, and these records do not establish cryptographic evidence validity.
func neutralMisbehavior() abci.Misbehavior {
	return abci.Misbehavior{Type: abci.MisbehaviorType_DUPLICATE_VOTE, Validator: abci.Validator{Address: bytes.Repeat([]byte{0xab}, 20), Power: 10}, Height: 7, Time: time.Unix(20, 123), TotalVotingPower: 40}
}

func TestMisbehaviorForwardedAcrossAllBlockMethods(t *testing.T) {
	input := []abci.Misbehavior{neutralMisbehavior()}
	expected := `[{"kind":"duplicate_vote","validator_address":"abababababababababababababababababababab","height":7,"time_seconds":20,"time_nanos":123,"power":10,"total_power":40}]`
	t.Setenv("DYTALLIX_BRIDGE_TEST_FACTS", expected)
	a := application{child: testChild(t, "misbehavior")}
	ctx := context.Background()
	now := time.Unix(30, 456)
	if _, err := a.PrepareProposal(ctx, &abci.RequestPrepareProposal{Height: 8, Time: now, Misbehavior: input, MaxTxBytes: 100}); err != nil {
		t.Fatal(err)
	}
	if r, err := a.ProcessProposal(ctx, &abci.RequestProcessProposal{Height: 8, Time: now, Hash: make([]byte, 32), Misbehavior: input}); err != nil || r.Status != abci.ResponseProcessProposal_ACCEPT {
		t.Fatalf("process: %v %v", r, err)
	}
	if _, err := a.FinalizeBlock(ctx, &abci.RequestFinalizeBlock{Height: 8, Time: now, Hash: make([]byte, 32), Misbehavior: input}); err != nil {
		t.Fatal(err)
	}
}

func TestMisbehaviorEmptyPreservesPayload(t *testing.T) {
	p := map[string]any{"height": int64(8)}
	before, _ := json.Marshal(p)
	if err := addMisbehavior(p, nil); err != nil {
		t.Fatal(err)
	}
	after, _ := json.Marshal(p)
	if !bytes.Equal(before, after) {
		t.Fatal("empty facts changed legacy serialization")
	}
}

func TestMisbehaviorRejectsInvalidRecordsWithoutPartialPayload(t *testing.T) {
	cases := map[string]func(*abci.Misbehavior){
		"unknown":                 func(m *abci.Misbehavior) { m.Type = abci.MisbehaviorType_UNKNOWN },
		"light_client":            func(m *abci.Misbehavior) { m.Type = abci.MisbehaviorType_LIGHT_CLIENT_ATTACK },
		"address":                 func(m *abci.Misbehavior) { m.Validator.Address = []byte{1} },
		"negative_height":         func(m *abci.Misbehavior) { m.Height = -1 },
		"zero_height":             func(m *abci.Misbehavior) { m.Height = 0 },
		"negative_time":           func(m *abci.Misbehavior) { m.Time = time.Unix(-1, 0) },
		"zero_power":              func(m *abci.Misbehavior) { m.Validator.Power = 0 },
		"negative_power":          func(m *abci.Misbehavior) { m.Validator.Power = -1 },
		"negative_total":          func(m *abci.Misbehavior) { m.TotalVotingPower = -1 },
		"power_over_total":        func(m *abci.Misbehavior) { m.Validator.Power = 41 },
		"total_over_engine_limit": func(m *abci.Misbehavior) { m.TotalVotingPower = types.MaxTotalVotingPower + 1 },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			bad := neutralMisbehavior()
			change(&bad)
			p := map[string]any{}
			if err := addMisbehavior(p, []abci.Misbehavior{neutralMisbehavior(), bad}); err == nil {
				t.Fatal("accepted invalid fact")
			}
			if _, ok := p["misbehavior"]; ok {
				t.Fatal("partially forwarded invalid facts")
			}
		})
	}
	input := make([]abci.Misbehavior, maxMisbehavior+1)
	for i := range input {
		input[i] = neutralMisbehavior()
	}
	if err := addMisbehavior(map[string]any{}, input); err == nil {
		t.Fatal("accepted oversized facts")
	}
	if err := addMisbehavior(map[string]any{}, input[:maxMisbehavior]); err != nil {
		t.Fatal(err)
	}
}

func TestUnsupportedMisbehaviorRejectedBeforeChild(t *testing.T) {
	a := application{}
	m := neutralMisbehavior()
	m.Type = abci.MisbehaviorType_LIGHT_CLIENT_ATTACK
	ctx := context.Background()
	now := time.Unix(30, 0)
	if _, err := a.PrepareProposal(ctx, &abci.RequestPrepareProposal{Height: 8, Time: now, Misbehavior: []abci.Misbehavior{m}}); err == nil {
		t.Fatal("prepare accepted unsupported facts")
	}
	if r, err := a.ProcessProposal(ctx, &abci.RequestProcessProposal{Height: 8, Time: now, Misbehavior: []abci.Misbehavior{m}}); err != nil || r.Status != abci.ResponseProcessProposal_REJECT {
		t.Fatalf("process: %v %v", r, err)
	}
	if _, err := a.FinalizeBlock(ctx, &abci.RequestFinalizeBlock{Height: 8, Time: now, Misbehavior: []abci.Misbehavior{m}}); err == nil {
		t.Fatal("finalize accepted unsupported facts")
	}
}
