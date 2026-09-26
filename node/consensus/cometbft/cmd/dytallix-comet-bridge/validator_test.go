package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
	cryptoencoding "github.com/cometbft/cometbft/crypto/encoding"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	"github.com/cometbft/cometbft/types"
)

func fixtureValidator(t *testing.T, power int64) (validator, abci.ValidatorUpdate) {
	t.Helper()
	key, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	pub := key.PubKey()
	proto, err := cryptoencoding.PubKeyToProto(pub)
	if err != nil {
		t.Fatal(err)
	}
	return validator{pub.Type(), base64.StdEncoding.EncodeToString(pub.Bytes()), power}, abci.ValidatorUpdate{PubKey: proto, Power: power}
}

func TestInitialValidatorsUseExplicitPowerAndEngineTotalBound(t *testing.T) {
	_, a := fixtureValidator(t, 1250000)
	_, b := fixtureValidator(t, 1)
	if got, err := validateInitialValidators([]abci.ValidatorUpdate{a, b}); err != nil || got[0].Power != 1250000 || got[1].Power != 1 {
		t.Fatalf("explicit power: %v %v", got, err)
	}
	a.Power = types.MaxTotalVotingPower
	if _, err := validateInitialValidators([]abci.ValidatorUpdate{a, b}); err == nil {
		t.Fatal("accepted total overflow")
	}
	b.Power = 0
	if _, err := validateInitialValidators([]abci.ValidatorUpdate{b}); err == nil {
		t.Fatal("accepted zero genesis power")
	}
	if _, err := validateInitialValidators([]abci.ValidatorUpdate{a, a}); err == nil {
		t.Fatal("accepted duplicate genesis address")
	}
	if _, err := validateInitialValidators(nil); err == nil {
		t.Fatal("accepted empty genesis")
	}
}

func TestValidatorUpdatesConvertAtomicRemovalAndAddition(t *testing.T) {
	old, _ := fixtureValidator(t, 0)
	next, _ := fixtureValidator(t, 1250001)
	got, err := convertValidatorUpdates([]validator{old, next})
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 2 || got[0].Power != 0 || got[1].Power != 1250001 {
		t.Fatal("changed absolute updates")
	}
	if _, err := convertValidatorUpdates([]validator{old, old}); err == nil {
		t.Fatal("accepted duplicate address")
	}
	next.Power = -1
	if _, err := convertValidatorUpdates([]validator{next}); err == nil {
		t.Fatal("accepted negative power")
	}
	next.Power = 1
	next.PubkeyBase64 = "AA=="
	if _, err := convertValidatorUpdates([]validator{next}); err == nil {
		t.Fatal("accepted invalid key")
	}
	if _, err := convertValidatorUpdates(make([]validator, maxValidatorUpdates+1)); err == nil {
		t.Fatal("accepted excessive updates")
	}
}

func TestFinalizeForwardsValidatorUpdatesAndDefaultsToEmpty(t *testing.T) {
	old, _ := fixtureValidator(t, 0)
	next, _ := fixtureValidator(t, 100)
	response, err := json.Marshal(map[string]any{"ok": true, "result": map[string]any{"app_hash": strings.Repeat("a", 64), "tx_results": []txResult{}, "validator_updates": []validator{old, next}}})
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("DYTALLIX_BRIDGE_TEST_RESPONSE", string(response))
	a := application{child: testChild(t, "updates")}
	got, err := a.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Height: 1, Time: time.Unix(1, 0), Hash: make([]byte, 32)})
	if err != nil || len(got.ValidatorUpdates) != 2 || got.ValidatorUpdates[0].Power != 0 || got.ValidatorUpdates[1].Power != 100 {
		t.Fatalf("forwarding: %v %v", got, err)
	}
	b := application{child: testChild(t, "count")}
	got, err = b.FinalizeBlock(context.Background(), &abci.RequestFinalizeBlock{Height: 1, Time: time.Unix(1, 0), Hash: make([]byte, 32)})
	if err != nil || len(got.ValidatorUpdates) != 0 {
		t.Fatalf("legacy empty updates: %v %v", got, err)
	}
}

func TestInitChainForwardsExactEvidenceLimits(t *testing.T) {
	_, v := fixtureValidator(t, 100)
	a := application{child: testChild(t, "evidence")}
	req := &abci.RequestInitChain{InitialHeight: 1, ChainId: "fixture", Validators: []abci.ValidatorUpdate{v}, ConsensusParams: &cmtproto.ConsensusParams{Validator: &cmtproto.ValidatorParams{PubKeyTypes: []string{mldsa65.KeyType}}, Evidence: &cmtproto.EvidenceParams{MaxAgeNumBlocks: 7, MaxAgeDuration: 3*time.Second + 5*time.Nanosecond}}}
	if _, err := a.InitChain(context.Background(), req); err != nil {
		t.Fatal(err)
	}
	req.ConsensusParams.Evidence = nil
	if _, err := a.InitChain(context.Background(), req); err == nil {
		t.Fatal("accepted missing evidence parameters")
	}
}
