package metricsfile

import (
	"time"

	"github.com/go-kit/kit/metrics"

	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	"github.com/cometbft/cometbft/types"
)

// SignSecondsMetric is the PQC signing time of this node's votes and
// proposals (metrics v1; P01, 10 October 2026).
const SignSecondsMetric = Prefix + "privval_sign_seconds"

// timedSigner records how long each successful vote and proposal signature
// takes. A refused signature, as a sentry's or endpoint's, is not recorded.
type timedSigner struct {
	types.PrivValidator
	seconds metrics.Histogram
}

// TimedSigner wraps signer so that each signature it makes is timed in
// seconds.
func TimedSigner(signer types.PrivValidator, seconds metrics.Histogram) types.PrivValidator {
	return timedSigner{PrivValidator: signer, seconds: seconds}
}

func (s timedSigner) SignVote(chainID string, vote *cmtproto.Vote) error {
	started := time.Now()
	err := s.PrivValidator.SignVote(chainID, vote)
	if err == nil {
		s.seconds.Observe(time.Since(started).Seconds())
	}
	return err
}

func (s timedSigner) SignProposal(chainID string, proposal *cmtproto.Proposal) error {
	started := time.Now()
	err := s.PrivValidator.SignProposal(chainID, proposal)
	if err == nil {
		s.seconds.Observe(time.Since(started).Seconds())
	}
	return err
}
