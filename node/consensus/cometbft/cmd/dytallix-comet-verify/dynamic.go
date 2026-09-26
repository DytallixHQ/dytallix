package main

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	rpchttp "github.com/cometbft/cometbft/rpc/client/http"
	coretypes "github.com/cometbft/cometbft/rpc/core/types"
	"github.com/cometbft/cometbft/types"
)

const maxDynamicValidators = 64
const maxDynamicHistory = 256

// checkedSet rejects invalid data before NewValidatorSet, which panics on invalid input.
func checkedSet(validators []*types.Validator) (*types.ValidatorSet, error) {
	if len(validators) == 0 || len(validators) > maxDynamicValidators {
		return nil, errors.New("validator count outside local resource bounds")
	}
	seen := make(map[string]bool, len(validators))
	var total int64
	for _, v := range validators {
		if v == nil || v.PubKey == nil || v.PubKey.Type() != mldsa65.KeyType || len(v.PubKey.Bytes()) != mldsa65.PubKeySize || v.VotingPower <= 0 || v.VotingPower > types.MaxTotalVotingPower-total {
			return nil, errors.New("invalid ML-DSA-65 validator or power total")
		}
		if err := v.ValidateBasic(); err != nil {
			return nil, err
		}
		address := string(v.Address)
		if seen[address] {
			return nil, errors.New("duplicate validator address")
		}
		seen[address] = true
		total += v.VotingPower
	}
	return types.NewValidatorSet(validators), nil
}

func dynamicGenesisSet(doc *types.GenesisDoc) (*types.ValidatorSet, error) {
	if len(doc.Validators) == 0 || len(doc.Validators) > maxDynamicValidators || doc.InitialHeight != 1 || doc.ConsensusParams == nil || len(doc.ConsensusParams.Validator.PubKeyTypes) != 1 || doc.ConsensusParams.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		return nil, errors.New("dynamic qualification requires ML-DSA-65 genesis at height 1")
	}
	validators := make([]*types.Validator, len(doc.Validators))
	for i, v := range doc.Validators {
		if v.PubKey == nil {
			return nil, errors.New("missing genesis public key")
		}
		validators[i] = &types.Validator{Address: v.Address, PubKey: v.PubKey, VotingPower: v.Power}
	}
	return checkedSet(validators)
}

// verifyLinkedCommit authenticates the current set against trusted genesis or the
// preceding signed header. RPC validator data is never its own trust anchor.
func verifyLinkedCommit(doc *types.GenesisDoc, initial, current *types.ValidatorSet, result, previous *coretypes.ResultCommit) error {
	if result == nil || result.Header == nil || result.Commit == nil {
		return errors.New("missing signed header")
	}
	if err := result.SignedHeader.ValidateBasic(doc.ChainID); err != nil {
		return err
	}
	if !bytes.Equal(current.Hash(), result.Header.ValidatorsHash) {
		return errors.New("header validator hash differs from supplied set")
	}
	if previous == nil {
		if result.Header.Height != 1 || !result.Header.LastBlockID.IsZero() || !bytes.Equal(current.Hash(), initial.Hash()) || !bytes.Equal(result.Header.NextValidatorsHash, initial.Hash()) {
			return errors.New("first header is not anchored to the initial validator set")
		}
		if len(doc.AppHash) > 0 && !bytes.Equal(doc.AppHash, result.Header.AppHash) {
			return errors.New("first header application hash differs from genesis")
		}
		if result.Header.Time.Before(doc.GenesisTime) {
			return errors.New("first header predates genesis")
		}
	} else {
		if previous.Header == nil || previous.Commit == nil || result.Header.Height != previous.Header.Height+1 || !result.Header.LastBlockID.Equals(previous.Commit.BlockID) {
			return errors.New("noncontiguous committed block history")
		}
		if !bytes.Equal(current.Hash(), previous.Header.NextValidatorsHash) {
			return errors.New("validator set is not authorized by the preceding signed header")
		}
		if !result.Header.Time.After(previous.Header.Time) {
			return errors.New("committed block time did not increase")
		}
	}
	if err := current.VerifyCommit(doc.ChainID, result.Commit.BlockID, result.Header.Height, result.Commit); err != nil {
		return fmt.Errorf("ML-DSA-65 commit verification failed: %w", err)
	}
	return nil
}

func verifyPriorApplicationResult(header *types.Header, prior *coretypes.ResultBlockResults) error {
	if prior == nil || prior.Height != header.Height-1 || len(prior.TxsResults) > 1000 {
		return errors.New("wrong prior application result height or count")
	}
	if len(prior.AppHash) != 32 || !bytes.Equal(header.AppHash, prior.AppHash) {
		return errors.New("header does not commit the prior application hash")
	}
	for _, tx := range prior.TxsResults {
		if tx == nil {
			return errors.New("missing prior transaction result")
		}
	}
	if !bytes.Equal(header.LastResultsHash, types.NewResults(prior.TxsResults).Hash()) {
		return errors.New("header does not commit prior transaction results")
	}
	return nil
}

func verifyDynamic(rpcURL, genesisFile string, target int64) (map[string]any, error) {
	if err := loopbackURL(rpcURL); err != nil {
		return nil, err
	}
	if target < 1 || target > maxDynamicHistory {
		return nil, fmt.Errorf("dynamic verification height must be 1..%d", maxDynamicHistory)
	}
	doc, err := types.GenesisDocFromFile(genesisFile)
	if err != nil {
		return nil, err
	}
	initial, err := dynamicGenesisSet(doc)
	if err != nil {
		return nil, err
	}
	client, err := rpchttp.New(rpcURL, "/websocket")
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	var previous *coretypes.ResultCommit
	var finalSet *types.ValidatorSet
	history := make([]map[string]any, 0, target)
	for height := int64(1); height <= target; height++ {
		result, err := client.Commit(ctx, &height)
		if err != nil {
			return nil, err
		}
		if result == nil || result.Header == nil || result.Commit == nil || result.Header.Height != height || result.Commit.Height != height {
			return nil, errors.New("RPC returned a different committed height")
		}
		page, perPage := 1, 100
		response, err := client.Validators(ctx, &height, &page, &perPage)
		if err != nil {
			return nil, err
		}
		if response == nil || response.BlockHeight != height || response.Total != len(response.Validators) || response.Count != len(response.Validators) {
			return nil, errors.New("incomplete RPC validator set")
		}
		set, err := checkedSet(response.Validators)
		if err != nil {
			return nil, err
		}
		if err := verifyLinkedCommit(doc, initial, set, result, previous); err != nil {
			return nil, fmt.Errorf("height %d: %w", height, err)
		}
		if height > 1 {
			priorHeight := height - 1
			prior, err := client.BlockResults(ctx, &priorHeight)
			if err != nil {
				return nil, err
			}
			if err := verifyPriorApplicationResult(result.Header, prior); err != nil {
				return nil, fmt.Errorf("height %d: %w", height, err)
			}
		}
		history = append(history, map[string]any{"height": height, "block_hash": hex.EncodeToString(result.Header.Hash()), "validators_hash": hex.EncodeToString(set.Hash()), "next_validators_hash": hex.EncodeToString(result.Header.NextValidatorsHash), "header_app_hash": hex.EncodeToString(result.Header.AppHash), "validators": set.Size(), "total_power": set.TotalVotingPower()})
		previous, finalSet = result, set
	}
	var signedPower int64
	var signatures int
	for _, sig := range previous.Commit.Signatures {
		if sig.BlockIDFlag != types.BlockIDFlagCommit {
			continue
		}
		_, v := finalSet.GetByAddress(sig.ValidatorAddress)
		if v == nil || len(sig.Signature) != mldsa65.SignatureSize {
			return nil, errors.New("unexpected verified commit signature")
		}
		signedPower += v.VotingPower
		signatures++
	}
	return map[string]any{
		"profile": "local-validator-lifecycle-qualification", "engine": "cometbft-v0.40.0", "chain_id": doc.ChainID,
		"height": target, "block_hash": hex.EncodeToString(previous.Header.Hash()), "header_app_hash": hex.EncodeToString(previous.Header.AppHash), "header_app_hash_height": target - 1,
		"validators": finalSet.Size(), "consensus_key_type": mldsa65.KeyType, "signed_power": signedPower, "total_power": finalSet.TotalVotingPower(), "block_signatures_verified": signatures,
		"signatures_verified": true, "validator_continuity_verified": true, "verified_from_height": 1, "verified_history": history,
		"application_commitments_verified": true, "application_execution_verified": false, "production_qualified": false,
	}, nil
}
