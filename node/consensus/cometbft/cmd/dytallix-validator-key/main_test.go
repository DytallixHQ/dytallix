package main

import (
	"encoding/base64"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/cometbft/cometbft/types"
)

// The node's proof_sign_bytes test asserts the same tuple encoding, with the
// same owner (runtime/validator_lifecycle_tests.rs,
// proof_sign_bytes_match_the_operator_tool).
const golden = "dytallix-validator-key-proof-v1\x00" +
	`["chain-1","register","validator-a","o\"w\\ñ<&>","KEY=",7,9,"20"]`

func TestProofBytesMatchTheNode(t *testing.T) {
	got, err := proofBytes("chain-1", "register", "validator-a", "o\"w\\ñ<&>", "KEY=", 7, 9, big.NewInt(20))
	if err != nil || string(got) != golden {
		t.Fatalf("proof bytes differ: %q %v", got, err)
	}
	for _, c := range []struct {
		operation, owner string
		amount           int64
	}{{"rotate", "owner", 1}, {"register", "owner", 0}, {"revoke", "owner", 1}, {"register", "has space", 1}, {"register", "", 1}} {
		if _, err := proofBytes("chain-1", c.operation, "v", c.owner, "KEY=", 1, 1, big.NewInt(c.amount)); err == nil {
			t.Fatalf("accepted %+v", c)
		}
	}
}

func fixture(t *testing.T) (dir, key, genesis string) {
	dir = t.TempDir()
	key = filepath.Join(dir, "priv_validator_key.json")
	genesis = filepath.Join(dir, "genesis.json")
	doc := types.GenesisDoc{ChainID: "chain-1", GenesisTime: time.Unix(1, 0).UTC()}
	if err := doc.SaveAs(genesis); err != nil {
		t.Fatal(err)
	}
	return dir, key, genesis
}

func TestGenerateWritesNewOwnerOnlyFiles(t *testing.T) {
	dir, key, _ := fixture(t)
	state := filepath.Join(dir, "priv_validator_state.json")
	out, err := generate(key, state)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{key, state} {
		info, err := os.Stat(path)
		if err != nil || info.Mode().Perm() != 0o600 {
			t.Fatalf("%s: %v %v", path, info, err)
		}
	}
	loaded, err := loadKey(key)
	if err != nil || out["public_key_base64"] != base64.StdEncoding.EncodeToString(loaded.PubKey.Bytes()) {
		t.Fatalf("generated key does not load: %v", err)
	}
	if _, err = generate(key, filepath.Join(dir, "other-state.json")); err == nil {
		t.Fatal("generate replaced an existing key")
	}
	if _, err = generate(filepath.Join(dir, "other-key.json"), state); err == nil {
		t.Fatal("generate replaced an existing state")
	}
	if _, err = generate("relative.json", filepath.Join(dir, "s.json")); err == nil {
		t.Fatal("generate accepted a relative path")
	}
}

func TestProofSignsTheBuiltPayloadForTheGenesisChain(t *testing.T) {
	dir, key, genesis := fixture(t)
	if _, err := generate(key, filepath.Join(dir, "state.json")); err != nil {
		t.Fatal(err)
	}
	in := proofInput{keyFile: key, genesis: genesis, operation: "rotate", validator: "validator-a",
		owner: "owner-a", amount: "0", nonce: 7, expiry: 90}
	out, err := proof(in)
	if err != nil {
		t.Fatal(err)
	}
	action := out["action"].(map[string]any)
	if out["chain_id"] != "chain-1" || action["type"] != "ValidatorRotateKey" || action["proof_expiry_height"] != "90" {
		t.Fatalf("unexpected output %v", out)
	}
	loaded, _ := loadKey(key)
	payload, _ := proofBytes("chain-1", "rotate", "validator-a", "owner-a",
		base64.StdEncoding.EncodeToString(loaded.PubKey.Bytes()), 7, 90, big.NewInt(0))
	signature := make([]byte, len(action["proof"].([]int)))
	for i, b := range action["proof"].([]int) {
		signature[i] = byte(b)
	}
	if !loaded.PubKey.VerifySignature(payload, signature) {
		t.Fatal("proof does not verify over the node's payload")
	}
	register := in
	register.operation, register.amount = "register", "20"
	if out, err = proof(register); err != nil || out["action"].(map[string]any)["amount_udgt"] != "20" {
		t.Fatalf("register: %v %v", out, err)
	}
	for _, bad := range []proofInput{
		func() proofInput { c := in; c.amount = "5"; return c }(),
		func() proofInput { c := in; c.amount = "007"; c.operation = "register"; return c }(),
		func() proofInput { c := in; c.owner = "two words"; return c }(),
	} {
		if _, err = proof(bad); err == nil {
			t.Fatalf("accepted %+v", bad)
		}
	}
	if err = os.Chmod(key, 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err = proof(in); err == nil {
		t.Fatal("signed with a group-readable key file")
	}
}
