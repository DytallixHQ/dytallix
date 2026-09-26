package main

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/privval"
)

func TestFixturePossessionProofSignsOnlyBoundIdentity(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "node4", "config")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	key, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	keyPath := filepath.Join(dir, "priv_validator_key.json")
	pv := privval.NewFilePV(key, keyPath, filepath.Join(dir, "state.json"))
	pv.Save()
	config := fixtureConfig{Profile: "cometbft-lifecycle-local-qualification", ChainID: "batch8-test"}
	config.Lifecycle.Profile = config.Profile
	config.Lifecycle.ChainID = config.ChainID
	config.Lifecycle.ApprovedOperators = map[string]string{"validator-4": "owner"}
	raw, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "application-config.json"), raw, 0o600); err != nil {
		t.Fatal(err)
	}
	tuple := []any{config.ChainID, "register", "validator-4", "owner", base64.StdEncoding.EncodeToString(key.PubKey().Bytes()), 1, 10, "100"}
	encoded, err := json.Marshal(tuple)
	if err != nil {
		t.Fatal(err)
	}
	payload := append([]byte(proofDomain), encoded...)
	path := filepath.Join(root, "proof.bin")
	if err := os.WriteFile(path, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	signature, err := signFixtureProof(keyPath, path)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := base64.StdEncoding.DecodeString(signature)
	if err != nil {
		t.Fatal(err)
	}
	if !key.PubKey().VerifySignature(payload, decoded) {
		t.Fatal("signature did not verify")
	}
	tuple[3] = "other-owner"
	encoded, _ = json.Marshal(tuple)
	if err := os.WriteFile(path, append([]byte(proofDomain), encoded...), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := signFixtureProof(keyPath, path); err == nil {
		t.Fatal("signed wrong owner")
	}
	if err := os.Chmod(keyPath, 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := signFixtureProof(keyPath, path); err == nil {
		t.Fatal("accepted accessible key file")
	}
}

func TestPossessionProofRejectsOtherDomainsAndOperations(t *testing.T) {
	config := fixtureConfig{ChainID: "batch8-test"}
	config.Lifecycle.ApprovedOperators = map[string]string{"validator-0": "owner"}
	for _, test := range []struct{ operation, amount string }{{"spend", "1"}, {"rotate", "1"}, {"register", "0"}, {"register", "01"}} {
		tuple, _ := json.Marshal([]any{config.ChainID, test.operation, "validator-0", "owner", "key", 1, 10, test.amount})
		if err := validateProof(append([]byte(proofDomain), tuple...), config, "key"); err == nil {
			t.Fatalf("accepted %s/%s", test.operation, test.amount)
		}
	}
	if err := validateProof([]byte("another-domain"), config, "key"); err == nil {
		t.Fatal("accepted another domain")
	}
}
