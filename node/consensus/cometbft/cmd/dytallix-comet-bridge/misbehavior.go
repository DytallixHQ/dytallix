package main

import (
	"encoding/hex"
	"errors"

	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/types"
)

const maxMisbehavior = 64

// These facts cross the private engine/application boundary. They are not raw
// evidence or standalone cryptographic proofs. Historical checks occur in Rust.
type misbehaviorFact struct {
	Kind             string `json:"kind"`
	ValidatorAddress string `json:"validator_address"`
	Height           uint64 `json:"height"`
	TimeSeconds      uint64 `json:"time_seconds"`
	TimeNanos        int32  `json:"time_nanos"`
	Power            int64  `json:"power"`
	TotalPower       int64  `json:"total_power"`
}

func addMisbehavior(payload map[string]any, input []abci.Misbehavior) error {
	if len(input) > maxMisbehavior {
		return errors.New("misbehavior exceeds local record limit")
	}
	if len(input) == 0 {
		return nil
	} // Preserve previous empty-block serialization.
	facts := make([]misbehaviorFact, len(input))
	for i, item := range input {
		if item.Type != abci.MisbehaviorType_DUPLICATE_VOTE {
			return errors.New("only duplicate-vote facts are supported in local qualification")
		}
		if len(item.Validator.Address) != 20 || item.Height <= 0 || item.Time.Unix() < 0 || item.Validator.Power <= 0 || item.TotalVotingPower <= 0 || item.Validator.Power > item.TotalVotingPower || item.TotalVotingPower > types.MaxTotalVotingPower {
			return errors.New("invalid duplicate-vote fact")
		}
		facts[i] = misbehaviorFact{Kind: "duplicate_vote", ValidatorAddress: hex.EncodeToString(item.Validator.Address), Height: uint64(item.Height), TimeSeconds: uint64(item.Time.Unix()), TimeNanos: int32(item.Time.Nanosecond()), Power: item.Validator.Power, TotalPower: item.TotalVotingPower}
	}
	payload["misbehavior"] = facts
	return nil
}
