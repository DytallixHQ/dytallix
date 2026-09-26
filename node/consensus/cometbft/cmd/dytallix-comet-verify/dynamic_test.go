package main

import (
	"bytes"
	"testing"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	cmtversion "github.com/cometbft/cometbft/proto/tendermint/version"
	coretypes "github.com/cometbft/cometbft/rpc/core/types"
	"github.com/cometbft/cometbft/types"
	"github.com/cometbft/cometbft/version"
)

func signedFixture(t *testing.T, key mldsa65.PrivKey, set, next *types.ValidatorSet, height int64, previous types.BlockID) *coretypes.ResultCommit {
	t.Helper()
	header := &types.Header{Version: cmtversion.Consensus{Block: version.BlockProtocol}, ChainID: "dynamic-test", Height: height, Time: time.Unix(height, 0), LastBlockID: previous, ValidatorsHash: set.Hash(), NextValidatorsHash: next.Hash(), AppHash: bytes.Repeat([]byte{byte(height)}, 32), ProposerAddress: key.PubKey().Address()}
	blockID := types.BlockID{Hash: header.Hash(), PartSetHeader: types.PartSetHeader{Total: 1, Hash: bytes.Repeat([]byte{4}, 32)}}
	vote := &types.Vote{Type: cmtproto.PrecommitType, Height: height, Round: 0, BlockID: blockID, Timestamp: header.Time, ValidatorAddress: key.PubKey().Address(), ValidatorIndex: 0}
	var err error
	vote.Signature, err = key.Sign(types.VoteSignBytes(header.ChainID, vote.ToProto()))
	if err != nil {
		t.Fatal(err)
	}
	return &coretypes.ResultCommit{SignedHeader: types.SignedHeader{Header: header, Commit: &types.Commit{Height: height, Round: 0, BlockID: blockID, Signatures: []types.CommitSig{vote.CommitSig()}}}}
}

func TestDynamicVerifierAnchorsRotationAndRejectsUnlinkedSet(t *testing.T) {
	old, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	next, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	initial := types.NewValidatorSet([]*types.Validator{types.NewValidator(old.PubKey(), 100)})
	rotated := types.NewValidatorSet([]*types.Validator{types.NewValidator(next.PubKey(), 100)})
	doc := &types.GenesisDoc{ChainID: "dynamic-test", GenesisTime: time.Unix(1, 0)}
	h1 := signedFixture(t, old, initial, initial, 1, types.BlockID{})
	h2 := signedFixture(t, old, initial, rotated, 2, h1.Commit.BlockID)
	h3 := signedFixture(t, next, rotated, rotated, 3, h2.Commit.BlockID)
	for _, pair := range []struct {
		current, previous *coretypes.ResultCommit
		set               *types.ValidatorSet
	}{{h1, nil, initial}, {h2, h1, initial}, {h3, h2, rotated}} {
		if err := verifyLinkedCommit(doc, initial, pair.set, pair.current, pair.previous); err != nil {
			t.Fatal(err)
		}
	}
	if err := verifyLinkedCommit(doc, initial, rotated, signedFixture(t, next, rotated, rotated, 2, h1.Commit.BlockID), h1); err == nil {
		t.Fatal("accepted unanchored RPC validator set")
	}
	if err := verifyLinkedCommit(doc, initial, rotated, signedFixture(t, next, rotated, rotated, 3, h1.Commit.BlockID), h2); err == nil {
		t.Fatal("accepted broken block linkage")
	}
	h3.Commit.Signatures[0].Signature[0] ^= 1
	if err := verifyLinkedCommit(doc, initial, rotated, h3, h2); err == nil {
		t.Fatal("accepted altered commit signature")
	}
}

func TestDynamicVerifierChecksApplicationCommitments(t *testing.T) {
	prior := &coretypes.ResultBlockResults{Height: 1, AppHash: bytes.Repeat([]byte{1}, 32), TxsResults: []*abci.ExecTxResult{{Code: 0, GasUsed: 2}}}
	header := &types.Header{Height: 2, AppHash: append([]byte(nil), prior.AppHash...), LastResultsHash: types.NewResults(prior.TxsResults).Hash()}
	if err := verifyPriorApplicationResult(header, prior); err != nil {
		t.Fatal(err)
	}
	prior.AppHash[0] ^= 1
	if err := verifyPriorApplicationResult(header, prior); err == nil {
		t.Fatal("accepted changed application hash")
	}
	prior.AppHash[0] ^= 1
	prior.TxsResults[0].GasUsed++
	if err := verifyPriorApplicationResult(header, prior); err == nil {
		t.Fatal("accepted changed transaction result")
	}
	prior.TxsResults[0] = nil
	if err := verifyPriorApplicationResult(header, prior); err == nil {
		t.Fatal("accepted missing transaction result")
	}
}
