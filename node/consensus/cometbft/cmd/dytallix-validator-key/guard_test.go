package main

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/cometbft/cometbft/privval"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
)

// The signing state host_install.py guard writes on a replacement validator
// (validator recovery, F17), at H = 173074. test_host_bundle.py asserts the
// guard writes these exact bytes, so the two cannot drift.
const guardFixture = "../../../../tools/mainnet-preparation/fixtures/signing-guard-state.json"
const guardHeight = 173074

// A guarded key refuses every proposal and vote at or below H, whatever the
// round, and signs again from H + 1: a replacement that starts a new chain
// from genesis cannot sign its height 1.
func TestGuardedStateRefusesEveryHeightAtOrBelowTheHalt(t *testing.T) {
	dir := t.TempDir()
	keyFile, stateFile := filepath.Join(dir, "key.json"), filepath.Join(dir, "state.json")
	if _, err := generate(keyFile, stateFile); err != nil {
		t.Fatal(err)
	}
	keyRaw, err := os.ReadFile(keyFile)
	if err != nil {
		t.Fatal(err)
	}
	guard, err := os.ReadFile(guardFixture)
	if err != nil {
		t.Fatal(err)
	}
	load := func() *privval.FilePV {
		pv, err := privval.LoadFilePVCheckedBytes(keyRaw, guard, keyFile, stateFile)
		if err != nil {
			t.Fatalf("the engine does not load the guarded state: %v", err)
		}
		return pv
	}
	if pv := load(); pv.LastSignState.Height != guardHeight {
		t.Fatalf("guarded height %d, want %d", pv.LastSignState.Height, guardHeight)
	}
	const chain = "dytallix-staging-2"
	for _, height := range []int64{1, 2, guardHeight - 1, guardHeight} {
		for _, round := range []int32{0, 1, 1000, 1<<31 - 1} {
			pv := load()
			for _, kind := range []cmtproto.SignedMsgType{cmtproto.PrevoteType, cmtproto.PrecommitType} {
				if err := pv.SignVote(chain, &cmtproto.Vote{Type: kind, Height: height, Round: round}); err == nil {
					t.Fatalf("signed a %v at height %d round %d", kind, height, round)
				}
			}
			if err := pv.SignProposal(chain, &cmtproto.Proposal{Type: cmtproto.ProposalType, Height: height, Round: round, PolRound: -1}); err == nil {
				t.Fatalf("signed a proposal at height %d round %d", height, round)
			}
		}
	}
	pv := load()
	if err := pv.SignProposal(chain, &cmtproto.Proposal{Type: cmtproto.ProposalType, Height: guardHeight + 1, PolRound: -1}); err != nil {
		t.Fatalf("refused the proposal at H + 1: %v", err)
	}
	if err := pv.SignVote(chain, &cmtproto.Vote{Type: cmtproto.PrevoteType, Height: guardHeight + 1}); err != nil {
		t.Fatalf("refused the prevote at H + 1: %v", err)
	}
	if err := pv.SignVote(chain, &cmtproto.Vote{Type: cmtproto.PrecommitType, Height: guardHeight + 1}); err != nil {
		t.Fatalf("refused the precommit at H + 1: %v", err)
	}
	// What it signed is kept: it cannot sign H + 1 again for another block.
	saved, err := os.ReadFile(stateFile)
	if err != nil {
		t.Fatal(err)
	}
	again, err := privval.LoadFilePVCheckedBytes(keyRaw, saved, keyFile, stateFile)
	if err != nil || again.LastSignState.Height != guardHeight+1 {
		t.Fatalf("the signed state was not kept: %v %v", again, err)
	}
}
