// Verifies a local qualification commit with the pinned engine's verifier.
package main

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"net/url"
	"os"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	rpchttp "github.com/cometbft/cometbft/rpc/client/http"
	"github.com/cometbft/cometbft/types"
)

func loopbackURL(raw string) error {
	u, err := url.Parse(raw)
	if err != nil {
		return err
	}
	if u.Scheme != "http" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || (u.Path != "" && u.Path != "/") {
		return errors.New("RPC URL must be a plain loopback HTTP endpoint")
	}
	ip := net.ParseIP(u.Hostname())
	if ip == nil || !ip.IsLoopback() || u.Port() == "" {
		return errors.New("RPC URL must use a numeric loopback address and explicit port")
	}
	return nil
}

func expectedValidators(doc *types.GenesisDoc) (map[string]types.GenesisValidator, error) {
	if doc.InitialHeight != 1 || len(doc.Validators) != 4 {
		return nil, errors.New("expected four-validator qualification genesis at initial height 1")
	}
	if doc.ConsensusParams == nil || len(doc.ConsensusParams.Validator.PubKeyTypes) != 1 || doc.ConsensusParams.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		return nil, errors.New("genesis must allow only ml_dsa_65")
	}
	expected := make(map[string]types.GenesisValidator, 4)
	for _, v := range doc.Validators {
		if v.PubKey == nil || v.PubKey.Type() != mldsa65.KeyType || v.Power != 10 {
			return nil, errors.New("genesis must contain ml_dsa_65 validators with power 10")
		}
		address := hex.EncodeToString(v.PubKey.Address())
		if _, exists := expected[address]; exists {
			return nil, errors.New("duplicate genesis validator")
		}
		expected[address] = v
	}
	return expected, nil
}

func verify(rpcURL, genesisFile string, height int64) (map[string]any, error) {
	if err := loopbackURL(rpcURL); err != nil {
		return nil, err
	}
	if height < 1 {
		return nil, errors.New("supply a positive explicit height")
	}
	doc, err := types.GenesisDocFromFile(genesisFile)
	if err != nil {
		return nil, err
	}
	expected, err := expectedValidators(doc)
	if err != nil {
		return nil, err
	}
	client, err := rpchttp.New(rpcURL, "/websocket")
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	result, err := client.Commit(ctx, &height)
	if err != nil {
		return nil, err
	}
	if result.Header == nil || result.Commit == nil || result.Header.Height != height || result.Commit.Height != height {
		return nil, errors.New("RPC returned a different or missing committed height")
	}
	if err := result.SignedHeader.ValidateBasic(doc.ChainID); err != nil {
		return nil, err
	}
	page, perPage := 1, 100
	response, err := client.Validators(ctx, &height, &page, &perPage)
	if err != nil {
		return nil, err
	}
	if response.BlockHeight != height || response.Total != len(expected) || len(response.Validators) != len(expected) {
		return nil, errors.New("RPC validator set differs from the fixed qualification set")
	}
	seen := make(map[string]bool, 4)
	for _, v := range response.Validators {
		if v == nil || v.PubKey == nil || v.PubKey.Type() != mldsa65.KeyType {
			return nil, errors.New("RPC returned a non-ML-DSA-65 consensus key")
		}
		address := hex.EncodeToString(v.Address)
		wanted, ok := expected[address]
		if !ok || seen[address] || v.VotingPower != wanted.Power || !bytes.Equal(v.PubKey.Bytes(), wanted.PubKey.Bytes()) || !bytes.Equal(v.PubKey.Address(), v.Address) {
			return nil, errors.New("RPC validator identity or power differs from genesis")
		}
		seen[address] = true
	}
	set := types.NewValidatorSet(response.Validators)
	if !bytes.Equal(set.Hash(), result.Header.ValidatorsHash) {
		return nil, errors.New("header validator hash differs from the verified validator set")
	}
	if err := set.VerifyCommit(doc.ChainID, result.Commit.BlockID, height, result.Commit); err != nil {
		return nil, fmt.Errorf("ML-DSA-65 commit verification failed: %w", err)
	}
	var signedPower int64
	var signatures int
	for _, sig := range result.Commit.Signatures {
		if sig.BlockIDFlag == types.BlockIDFlagCommit {
			v, ok := expected[hex.EncodeToString(sig.ValidatorAddress)]
			if !ok || len(sig.Signature) != mldsa65.SignatureSize {
				return nil, errors.New("unexpected verified commit signature")
			}
			signedPower += v.Power
			signatures++
		}
	}
	if signedPower <= 2*set.TotalVotingPower()/3 {
		return nil, errors.New("verified commit does not have strictly more than two-thirds voting power")
	}
	return map[string]any{
		"profile": "local-development-qualification", "engine": "cometbft-v0.40.0", "chain_id": doc.ChainID,
		"height": height, "block_hash": hex.EncodeToString(result.Header.Hash()), "header_app_hash": hex.EncodeToString(result.Header.AppHash),
		"header_app_hash_height": height - 1, "validators": len(expected), "consensus_key_type": mldsa65.KeyType,
		"signed_power": signedPower, "total_power": set.TotalVotingPower(), "block_signatures_verified": signatures,
		"signatures_verified": true, "production_qualified": false,
	}, nil
}

func run() error {
	rpc := flag.String("rpc", "", "numeric loopback HTTP RPC endpoint")
	genesis := flag.String("genesis", "", "expected local engine genesis file")
	height := flag.Int64("height", 0, "explicit committed height to verify")
	dynamic := flag.Bool("dynamic", false, "verify all headers and validator changes from trusted genesis; maximum 256 heights")
	flag.Parse()
	if flag.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	var result map[string]any
	var err error
	if *dynamic {
		result, err = verifyDynamic(*rpc, *genesis, *height)
	} else {
		result, err = verify(*rpc, *genesis, *height)
	}
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(result)
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
