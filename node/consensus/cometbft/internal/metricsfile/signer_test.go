package metricsfile

import (
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
)

// stubSigner signs or refuses every request.
type stubSigner struct{ refuse bool }

func (stubSigner) GetPubKey() (crypto.PubKey, error) { return nil, nil }

func (s stubSigner) SignVote(string, *cmtproto.Vote) error {
	if s.refuse {
		return errors.New("refused")
	}
	return nil
}

func (s stubSigner) SignProposal(string, *cmtproto.Proposal) error {
	if s.refuse {
		return errors.New("refused")
	}
	return nil
}

// Each successful vote and proposal signature is timed; a refused one, as a
// sentry's, is not.
func TestTimedSignerRecordsEachSignature(t *testing.T) {
	registry := NewRegistry()
	seconds := registry.Histogram(SignSecondsMetric)
	signer := TimedSigner(stubSigner{}, seconds)
	if signer.SignVote("c", &cmtproto.Vote{}) != nil || signer.SignProposal("c", &cmtproto.Proposal{}) != nil {
		t.Fatal("a signature failed")
	}
	refusing := TimedSigner(stubSigner{refuse: true}, seconds)
	if refusing.SignVote("c", &cmtproto.Vote{}) == nil {
		t.Fatal("a refusal passed")
	}
	var out strings.Builder
	if err := registry.Write(&out, "engine", time.Unix(1, 0)); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out.String(), SignSecondsMetric+"_count 2") {
		t.Fatal(out.String())
	}
}
