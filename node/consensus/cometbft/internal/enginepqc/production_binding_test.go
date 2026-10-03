package enginepqc

import (
	"strings"
	"testing"

	"github.com/cometbft/cometbft/privval"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	"github.com/cometbft/cometbft/types"
)

func TestProductionBindingCodecIsCanonical(t *testing.T) {
	digest := strings.Repeat("ab", 32)
	raw := `{"schema":1,"role":"sentry","chain_id":"c","config_sha256":"` + digest + `","genesis_sha256":"` + digest +
		`","transport_sha256":"` + digest + `","peer_public_key_sha256":"` + digest + `","validator_public_key_sha256":"` + digest + `"}`
	binding, err := DecodeProductionBinding([]byte(raw))
	if err != nil || binding.Role != RoleSentry || binding.ChainID != "c" {
		t.Fatalf("canonical binding refused: %v", err)
	}
	for name, changed := range map[string]string{
		"spaced":   strings.Replace(raw, `"schema":1`, `"schema": 1`, 1),
		"unknown":  strings.Replace(raw, `{"schema":1`, `{"schema":1,"note":"x"`, 1),
		"trailing": raw + `{}`,
		"reorder":  strings.Replace(raw, `"schema":1,"role":"sentry"`, `"role":"sentry","schema":1`, 1),
	} {
		if _, err := DecodeProductionBinding([]byte(changed)); err == nil {
			t.Fatalf("%s binding accepted", name)
		}
	}
	if ValidateProductionBinding(nil, binding) == nil {
		t.Fatal("binding without a production runtime accepted")
	}
}

func TestDevelopmentBuildRefusesTheProductionProfile(t *testing.T) {
	if ProductionBuild {
		t.Skip("development build only")
	}
	if _, err := Load("/nonexistent", ProductionProfile); err == nil || !strings.Contains(err.Error(), "no production transport profile") {
		t.Fatalf("production profile in a development build: %v", err)
	}
}

func TestASentryOrEndpointSignerRefusesEverySignature(t *testing.T) {
	validator := privval.GenFilePV("", "")
	runtime := &Runtime{Validator: validator}
	for _, role := range []string{RoleSentry, RoleEndpoint} {
		signer := SignerForRole(runtime, role)
		public, err := signer.GetPubKey()
		if err != nil || !public.Equals(validator.Key.PubKey) {
			t.Fatalf("%s identity: %v", role, err)
		}
		vote := &cmtproto.Vote{Type: cmtproto.PrevoteType, Height: 1}
		proposal := &cmtproto.Proposal{Type: cmtproto.ProposalType, Height: 1}
		if err := signer.SignVote("c", vote); err == nil || len(vote.Signature) != 0 {
			t.Fatalf("a %s signed a vote", role)
		}
		if err := signer.SignProposal("c", proposal); err == nil || len(proposal.Signature) != 0 {
			t.Fatalf("a %s signed a proposal", role)
		}
	}
	if SignerForRole(runtime, RoleValidator) != types.PrivValidator(validator) {
		t.Fatal("a validator does not sign with its key")
	}
}

func TestOnlyASentryOrEndpointKeyIsRefusedInGenesis(t *testing.T) {
	key := privval.GenFilePV("", "").Key.PubKey
	genesis := &types.GenesisDoc{Validators: []types.GenesisValidator{{PubKey: key, Power: 1}}}
	other := privval.GenFilePV("", "").Key.PubKey
	for role, cases := range map[string][2]bool{
		// {genesis key allowed, outside key allowed}
		RoleValidator: {true, true},
		RoleSentry:    {false, true},
		RoleEndpoint:  {false, true},
	} {
		if (roleMatchesGenesis(genesis, key.Bytes(), role) == nil) != cases[0] || (roleMatchesGenesis(genesis, other.Bytes(), role) == nil) != cases[1] {
			t.Fatalf("%s role rule", role)
		}
	}
}
