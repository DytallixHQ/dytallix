package main

import (
	"context"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
	cryptoencoding "github.com/cometbft/cometbft/crypto/encoding"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/types"
)

const maxValidators = 64
const maxValidatorUpdates = 2 * maxValidators

type application struct{ child *child }

var _ abci.Application = (*application)(nil)

type validator struct {
	PubkeyType   string `json:"pubkey_type"`
	PubkeyBase64 string `json:"pubkey_base64"`
	Power        int64  `json:"power"`
}
type txResult struct {
	Code      uint32 `json:"code"`
	GasWanted int64  `json:"gas_wanted"`
	GasUsed   int64  `json:"gas_used"`
	Log       string `json:"log"`
	Data      string `json:"data"`
}

func parseHash(s string, allowEmpty bool) ([]byte, error) {
	if allowEmpty && s == "" {
		return nil, nil
	}
	if len(s) != 64 || s != strings.ToLower(s) {
		return nil, errors.New("application hash must be 32-byte lowercase hex")
	}
	return hex.DecodeString(s)
}
func encodeTxs(txs [][]byte) []string {
	result := make([]string, len(txs))
	for i, tx := range txs {
		result[i] = base64.StdEncoding.EncodeToString(tx)
	}
	return result
}
func decodeTxs(txs []string) ([][]byte, error) {
	result := make([][]byte, len(txs))
	for i, tx := range txs {
		var err error
		result[i], err = base64.StdEncoding.Strict().DecodeString(tx)
		if err != nil {
			return nil, err
		}
	}
	return result, nil
}
func blockPayload(height int64, timestamp time.Time, txs [][]byte) (map[string]any, error) {
	if height <= 0 || timestamp.Unix() < 0 {
		return nil, errors.New("invalid finalized height or time")
	}
	return map[string]any{"height": height, "time_seconds": timestamp.Unix(), "time_nanos": timestamp.Nanosecond(), "txs": encodeTxs(txs)}, nil
}
func convertResult(r txResult) (*abci.ExecTxResult, error) {
	if r.GasWanted < 0 || r.GasUsed < 0 {
		return nil, errors.New("negative application gas result")
	}
	data, err := base64.StdEncoding.Strict().DecodeString(r.Data)
	if err != nil {
		return nil, err
	}
	return &abci.ExecTxResult{Code: r.Code, GasWanted: r.GasWanted, GasUsed: r.GasUsed, Log: r.Log, Data: data}, nil
}

func (a *application) Info(ctx context.Context, _ *abci.RequestInfo) (*abci.ResponseInfo, error) {
	var result struct {
		Height     int64  `json:"height"`
		AppHash    string `json:"app_hash"`
		AppVersion uint64 `json:"app_version"`
	}
	if err := a.child.call(ctx, "info", struct{}{}, &result); err != nil {
		return nil, err
	}
	if result.Height < 0 {
		return nil, errors.New("negative committed application height")
	}
	hash, err := parseHash(result.AppHash, result.Height == 0)
	if err != nil {
		return nil, err
	}
	return &abci.ResponseInfo{Data: "Dytallix local qualification", Version: "batch9", AppVersion: result.AppVersion, LastBlockHeight: result.Height, LastBlockAppHash: hash}, nil
}

func (a *application) InitChain(ctx context.Context, req *abci.RequestInitChain) (*abci.ResponseInitChain, error) {
	if req.InitialHeight != 1 {
		return nil, errors.New("local qualification requires initial height 1")
	}
	if req.ConsensusParams == nil || req.ConsensusParams.Validator == nil ||
		len(req.ConsensusParams.Validator.PubKeyTypes) != 1 || req.ConsensusParams.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		return nil, errors.New("consensus must allow only ml_dsa_65")
	}
	if req.ConsensusParams.Abci != nil && req.ConsensusParams.Abci.VoteExtensionsEnableHeight != 0 {
		return nil, errors.New("vote extensions are disabled in local qualification")
	}
	validators, err := validateInitialValidators(req.Validators)
	if err != nil {
		return nil, err
	}
	var result struct {
		AppHash string `json:"app_hash"`
	}
	payload := map[string]any{"chain_id": req.ChainId, "initial_height": req.InitialHeight, "app_state_bytes": base64.StdEncoding.EncodeToString(req.AppStateBytes), "validators": validators}
	if req.ConsensusParams.Evidence == nil || req.ConsensusParams.Evidence.MaxAgeNumBlocks <= 0 || req.ConsensusParams.Evidence.MaxAgeDuration <= 0 {
		return nil, errors.New("explicit positive engine evidence limits are required")
	}
	payload["evidence_max_age_blocks"] = req.ConsensusParams.Evidence.MaxAgeNumBlocks
	payload["evidence_max_age_seconds"] = int64(req.ConsensusParams.Evidence.MaxAgeDuration / time.Second)
	payload["evidence_max_age_nanos"] = int32(req.ConsensusParams.Evidence.MaxAgeDuration % time.Second)

	if err := a.child.call(ctx, "init_chain", payload, &result); err != nil {
		return nil, err
	}
	hash, err := parseHash(result.AppHash, false)
	if err != nil {
		return nil, err
	}
	return &abci.ResponseInitChain{AppHash: hash, Validators: req.Validators}, nil
}

func (a *application) CheckTx(ctx context.Context, req *abci.RequestCheckTx) (*abci.ResponseCheckTx, error) {
	kind := "new"
	if req.Type == abci.CheckTxType_Recheck {
		kind = "recheck"
	} else if req.Type != abci.CheckTxType_New {
		return nil, errors.New("unknown CheckTx type")
	}
	var result txResult
	if err := a.child.call(ctx, "check_tx", map[string]any{"tx": base64.StdEncoding.EncodeToString(req.Tx), "type": kind}, &result); err != nil {
		return nil, err
	}
	r, err := convertResult(result)
	if err != nil {
		return nil, err
	}
	return &abci.ResponseCheckTx{Code: r.Code, GasWanted: r.GasWanted, GasUsed: r.GasUsed, Log: r.Log, Data: r.Data}, nil
}

func (a *application) PrepareProposal(ctx context.Context, req *abci.RequestPrepareProposal) (*abci.ResponsePrepareProposal, error) {
	payload, err := blockPayload(req.Height, req.Time, req.Txs)
	if err != nil {
		return nil, err
	}
	if err := addMisbehavior(payload, req.Misbehavior); err != nil {
		return nil, err
	}
	if req.MaxTxBytes < 0 {
		return nil, errors.New("negative proposal byte limit")
	}
	payload["max_tx_bytes"] = req.MaxTxBytes
	var result struct {
		Txs []string `json:"txs"`
	}
	if err := a.child.call(ctx, "prepare_proposal", payload, &result); err != nil {
		return nil, err
	}
	txs, err := decodeTxs(result.Txs)
	if err != nil {
		return nil, err
	}
	var bytes int64
	for _, tx := range txs {
		if int64(len(tx)) > req.MaxTxBytes-bytes {
			return nil, errors.New("application exceeded proposal byte limit")
		}
		bytes += int64(len(tx))
	}
	return &abci.ResponsePrepareProposal{Txs: txs}, nil
}

func (a *application) ProcessProposal(ctx context.Context, req *abci.RequestProcessProposal) (*abci.ResponseProcessProposal, error) {
	payload, err := blockPayload(req.Height, req.Time, req.Txs)
	if err != nil {
		return nil, err
	}
	if err := addMisbehavior(payload, req.Misbehavior); err != nil {
		return &abci.ResponseProcessProposal{Status: abci.ResponseProcessProposal_REJECT}, nil
	}
	if len(req.Hash) != 32 {
		return nil, errors.New("decided engine hash must be 32 bytes")
	}
	payload["hash"] = hex.EncodeToString(req.Hash)
	var result struct {
		Accept bool `json:"accept"`
	}
	if err := a.child.call(ctx, "process_proposal", payload, &result); err != nil {
		return nil, err
	}
	status := abci.ResponseProcessProposal_REJECT
	if result.Accept {
		status = abci.ResponseProcessProposal_ACCEPT
	}
	return &abci.ResponseProcessProposal{Status: status}, nil
}

func (a *application) FinalizeBlock(ctx context.Context, req *abci.RequestFinalizeBlock) (*abci.ResponseFinalizeBlock, error) {
	payload, err := blockPayload(req.Height, req.Time, req.Txs)
	if err != nil {
		return nil, err
	}
	if err := addMisbehavior(payload, req.Misbehavior); err != nil {
		return nil, err
	}
	if len(req.Hash) != 32 {
		return nil, errors.New("decided engine hash must be 32 bytes")
	}
	payload["hash"] = hex.EncodeToString(req.Hash)
	var result struct {
		AppHash          string      `json:"app_hash"`
		TxResults        []txResult  `json:"tx_results"`
		ValidatorUpdates []validator `json:"validator_updates"`
	}
	if err := a.child.call(ctx, "finalize_block", payload, &result); err != nil {
		return nil, err
	}
	if len(result.TxResults) != len(req.Txs) {
		return nil, fmt.Errorf("application returned %d results for %d transactions", len(result.TxResults), len(req.Txs))
	}
	hash, err := parseHash(result.AppHash, false)
	if err != nil {
		return nil, err
	}
	results := make([]*abci.ExecTxResult, len(result.TxResults))
	for i, r := range result.TxResults {
		results[i], err = convertResult(r)
		if err != nil {
			return nil, err
		}
	}
	updates, err := convertValidatorUpdates(result.ValidatorUpdates)
	if err != nil {
		return nil, err
	}
	// The application validates the resulting scheduled set. The engine applies it at H+2.
	return &abci.ResponseFinalizeBlock{AppHash: hash, TxResults: results, ValidatorUpdates: updates}, nil
}

func (a *application) Commit(ctx context.Context, _ *abci.RequestCommit) (*abci.ResponseCommit, error) {
	var result struct{}
	if err := a.child.call(ctx, "commit", struct{}{}, &result); err != nil {
		return nil, err
	}
	return &abci.ResponseCommit{RetainHeight: 0}, nil
}

func (a *application) Query(ctx context.Context, req *abci.RequestQuery) (*abci.ResponseQuery, error) {
	if req.Prove {
		return &abci.ResponseQuery{Code: 1, Log: "application proofs are not qualified"}, nil
	}
	var result struct {
		Code   uint32 `json:"code"`
		Log    string `json:"log"`
		Height int64  `json:"height"`
		Value  string `json:"value"`
	}
	payload := map[string]any{"path": req.Path, "data": base64.StdEncoding.EncodeToString(req.Data), "height": req.Height, "prove": req.Prove}
	if err := a.child.call(ctx, "query", payload, &result); err != nil {
		return nil, err
	}
	value, err := base64.StdEncoding.Strict().DecodeString(result.Value)
	if err != nil {
		return nil, err
	}
	return &abci.ResponseQuery{Code: result.Code, Log: result.Log, Height: result.Height, Value: value}, nil
}

func (*application) InsertTx(context.Context, *abci.RequestInsertTx) (*abci.ResponseInsertTx, error) {
	return nil, errors.New("use the engine flood mempool; application mempool is disabled")
}
func (*application) ReapTxs(context.Context, *abci.RequestReapTxs) (*abci.ResponseReapTxs, error) {
	return nil, errors.New("use the engine flood mempool; application mempool is disabled")
}
func (*application) ExtendVote(context.Context, *abci.RequestExtendVote) (*abci.ResponseExtendVote, error) {
	return nil, errors.New("vote extensions are disabled")
}
func (*application) VerifyVoteExtension(context.Context, *abci.RequestVerifyVoteExtension) (*abci.ResponseVerifyVoteExtension, error) {
	return &abci.ResponseVerifyVoteExtension{Status: abci.ResponseVerifyVoteExtension_REJECT}, nil
}
func (*application) ListSnapshots(context.Context, *abci.RequestListSnapshots) (*abci.ResponseListSnapshots, error) {
	return &abci.ResponseListSnapshots{}, nil
}
func (*application) OfferSnapshot(context.Context, *abci.RequestOfferSnapshot) (*abci.ResponseOfferSnapshot, error) {
	return &abci.ResponseOfferSnapshot{Result: abci.ResponseOfferSnapshot_REJECT}, nil
}
func (*application) LoadSnapshotChunk(context.Context, *abci.RequestLoadSnapshotChunk) (*abci.ResponseLoadSnapshotChunk, error) {
	return &abci.ResponseLoadSnapshotChunk{}, nil
}
func (*application) ApplySnapshotChunk(context.Context, *abci.RequestApplySnapshotChunk) (*abci.ResponseApplySnapshotChunk, error) {
	return &abci.ResponseApplySnapshotChunk{Result: abci.ResponseApplySnapshotChunk_REJECT_SNAPSHOT}, nil
}

// validateInitialValidators bounds decoding before asking the application to match genesis.
func validateInitialValidators(input []abci.ValidatorUpdate) ([]validator, error) {
	if len(input) == 0 || len(input) > maxValidators {
		return nil, errors.New("initial validator count outside local resource bounds")
	}
	output := make([]validator, len(input))
	seen := make(map[string]bool, len(input))
	var total int64
	for i, v := range input {
		key, err := cryptoencoding.PubKeyFromProto(v.PubKey)
		if err != nil {
			return nil, err
		}
		if key.Type() != mldsa65.KeyType || len(key.Bytes()) != mldsa65.PubKeySize || v.Power <= 0 || v.Power > types.MaxTotalVotingPower-total {
			return nil, errors.New("invalid initial ML-DSA-65 validator or voting power total")
		}
		address := string(key.Address())
		if seen[address] {
			return nil, errors.New("duplicate consensus address")
		}
		seen[address] = true
		total += v.Power
		output[i] = validator{key.Type(), base64.StdEncoding.EncodeToString(key.Bytes()), v.Power}
	}
	return output, nil
}

func convertValidatorUpdates(input []validator) ([]abci.ValidatorUpdate, error) {
	if len(input) > maxValidatorUpdates {
		return nil, errors.New("validator update count exceeds local resource bound")
	}
	output := make([]abci.ValidatorUpdate, len(input))
	seen := make(map[string]bool, len(input))
	for i, v := range input {
		if v.PubkeyType != mldsa65.KeyType || len(v.PubkeyBase64) != base64.StdEncoding.EncodedLen(mldsa65.PubKeySize) || v.Power < 0 || v.Power > types.MaxTotalVotingPower {
			return nil, errors.New("invalid validator update type or power")
		}
		raw, err := base64.StdEncoding.Strict().DecodeString(v.PubkeyBase64)
		if err != nil || base64.StdEncoding.EncodeToString(raw) != v.PubkeyBase64 {
			return nil, errors.New("invalid canonical validator public key encoding")
		}
		key, err := mldsa65.NewPubKeyFromBytes(raw)
		if err != nil {
			return nil, err
		}
		address := string(key.Address())
		if seen[address] {
			return nil, errors.New("duplicate validator update address")
		}
		seen[address] = true
		protoKey, err := cryptoencoding.PubKeyToProto(key)
		if err != nil {
			return nil, err
		}
		output[i] = abci.ValidatorUpdate{PubKey: protoKey, Power: v.Power}
	}
	return output, nil
}
