package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strconv"

	"github.com/cometbft/cometbft/types"
)

type lifecycleConfig struct {
	Version                 int               `json:"version"`
	Profile                 string            `json:"profile"`
	ChainID                 string            `json:"chain_id"`
	ApprovedOperators       map[string]string `json:"approved_operators"`
	MinSelfBond             string            `json:"min_self_bond"`
	MaxActive               int               `json:"max_active"`
	EvidenceMaxAgeBlocks    int64             `json:"evidence_max_age_blocks"`
	EvidenceMaxAgeSeconds   int64             `json:"evidence_max_age_seconds"`
	ProcessingMarginBlocks  int64             `json:"processing_margin_blocks"`
	ProcessingMarginSeconds int64             `json:"processing_margin_seconds"`
}

func fixtureLifecycle(operatorsFile string, app []byte, chainID string) (*lifecycleConfig, []int64, error) {
	raw, err := os.ReadFile(operatorsFile)
	if err != nil {
		return nil, nil, err
	}
	if len(raw) > 16384 {
		return nil, nil, errors.New("operator fixture exceeds 16 KiB")
	}
	var operators map[string]string
	if err := json.Unmarshal(raw, &operators); err != nil {
		return nil, nil, err
	}
	if len(operators) < 5 || len(operators) > 8 {
		return nil, nil, errors.New("lifecycle fixture requires five to eight approved operators")
	}
	for id, owner := range operators {
		if id == "" || owner == "" || len(id) > 128 || len(owner) > 128 {
			return nil, nil, errors.New("invalid fixture operator identity")
		}
	}
	for i := 0; i < 5; i++ {
		if operators[fmt.Sprintf("validator-%d", i)] == "" {
			return nil, nil, errors.New("fixture requires operators validator-0 through validator-4")
		}
	}
	var native struct {
		RewardV2 struct {
			Positions []struct {
				Owner     string `json:"owner"`
				Validator string `json:"validator"`
				Amount    string `json:"amount_udgt"`
			} `json:"positions"`
		} `json:"reward_v2"`
	}
	if err := json.Unmarshal(app, &native); err != nil {
		return nil, nil, err
	}
	powers := make([]int64, 4)
	self := make([]int64, 4)
	var total int64
	for _, position := range native.RewardV2.Positions {
		amount, err := strconv.ParseInt(position.Amount, 10, 64)
		if err != nil || amount <= 0 || strconv.FormatInt(amount, 10) != position.Amount || amount > types.MaxTotalVotingPower-total {
			return nil, nil, errors.New("invalid fixture funded principal")
		}
		matched := false
		for i := 0; i < 4; i++ {
			id := fmt.Sprintf("validator-%d", i)
			if position.Validator == id {
				powers[i] += amount
				if position.Owner == operators[id] {
					self[i] += amount
				}
				matched = true
				break
			}
		}
		if !matched {
			return nil, nil, errors.New("initial lifecycle positions must target validator-0 through validator-3")
		}
		total += amount
	}
	for i := 0; i < 4; i++ {
		if powers[i] <= 0 || self[i] < 10 {
			return nil, nil, errors.New("initial validators require funded operator self-bond of at least 10 uDGT")
		}
	}
	return &lifecycleConfig{Version: 1, Profile: "cometbft-lifecycle-local-qualification", ChainID: chainID, ApprovedOperators: operators, MinSelfBond: "10", MaxActive: 8, EvidenceMaxAgeBlocks: 3, EvidenceMaxAgeSeconds: 3, ProcessingMarginBlocks: 1, ProcessingMarginSeconds: 1}, powers, nil
}
