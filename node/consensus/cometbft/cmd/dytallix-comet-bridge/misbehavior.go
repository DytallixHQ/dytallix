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
		// Both kinds CometBFT verifies are forwarded; the application records
		// them, or penalizes a duplicate vote under its penalty profile. A
		// refusal here would stop the chain (P01, 27 September 2026).
		var kind string
		switch item.Type {
		case abci.MisbehaviorType_DUPLICATE_VOTE:
			kind = "duplicate_vote"
		case abci.MisbehaviorType_LIGHT_CLIENT_ATTACK:
			kind = "light_client_attack"
		default:
			return errors.New("unknown misbehavior type")
		}
		if len(item.Validator.Address) != 20 || item.Height <= 0 || item.Time.Unix() < 0 || item.Validator.Power <= 0 || item.TotalVotingPower <= 0 || item.Validator.Power > item.TotalVotingPower || item.TotalVotingPower > types.MaxTotalVotingPower {
			return errors.New("invalid misbehavior fact")
		}
		facts[i] = misbehaviorFact{Kind: kind, ValidatorAddress: hex.EncodeToString(item.Validator.Address), Height: uint64(item.Height), TimeSeconds: uint64(item.Time.Unix()), TimeNanos: int32(item.Time.Nanosecond()), Power: item.Validator.Power, TotalPower: item.TotalVotingPower}
	}
	payload["misbehavior"] = facts
	return nil
}
