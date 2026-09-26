// Signs only domain-bound possession proofs with disposable local fixture keys.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"strconv"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/privval"
)

const proofDomain = "dytallix-validator-key-proof-v1\x00"

type fixtureConfig struct {
	Profile   string `json:"profile"`
	ChainID   string `json:"chain_id"`
	Lifecycle struct {
		Profile           string            `json:"profile"`
		ChainID           string            `json:"chain_id"`
		ApprovedOperators map[string]string `json:"approved_operators"`
	} `json:"lifecycle"`
}

func boundedFile(path string, limit int64, private bool) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > limit || (private && info.Mode().Perm() != 0o600) {
		return nil, errors.New("fixture file must be regular, bounded, and private when required")
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(raw)) > limit {
		return nil, errors.New("fixture file grew beyond size bound")
	}
	return raw, nil
}

func validateProof(payload []byte, config fixtureConfig, encodedKey string) error {
	if !bytes.HasPrefix(payload, []byte(proofDomain)) {
		return errors.New("unsupported possession proof domain")
	}
	var tuple []json.RawMessage
	if err := json.Unmarshal(payload[len(proofDomain):], &tuple); err != nil || len(tuple) != 8 {
		return errors.New("invalid possession proof tuple")
	}
	var chain, operation, id, owner, key, amount string
	for _, entry := range []struct {
		index int
		value *string
	}{{0, &chain}, {1, &operation}, {2, &id}, {3, &owner}, {4, &key}, {7, &amount}} {
		if json.Unmarshal(tuple[entry.index], entry.value) != nil {
			return errors.New("invalid possession proof string")
		}
	}
	for _, index := range []int{5, 6} {
		if _, err := strconv.ParseUint(string(tuple[index]), 10, 64); err != nil {
			return errors.New("invalid possession proof nonce or expiry")
		}
	}
	if chain != config.ChainID || owner == "" || config.Lifecycle.ApprovedOperators[id] != owner || key != encodedKey {
		return errors.New("proof does not match fixture chain, operator, or public key")
	}
	value, ok := new(big.Int).SetString(amount, 10)
	if !ok || value.Sign() < 0 || value.BitLen() > 128 || value.String() != amount {
		return errors.New("invalid possession proof amount")
	}
	if (operation == "register" && value.Sign() > 0) || (operation == "rotate" && value.Sign() == 0) {
		return nil
	}
	return errors.New("unsupported proof operation or amount")
}

func signFixtureProof(keyPath, payloadPath string) (string, error) {
	if !filepath.IsAbs(keyPath) || filepath.Clean(keyPath) != keyPath || filepath.Base(keyPath) != "priv_validator_key.json" || filepath.Base(filepath.Dir(keyPath)) != "config" {
		return "", errors.New("key must be an absolute fixture validator key path")
	}
	nodeDir := filepath.Dir(filepath.Dir(keyPath))
	nodeName := filepath.Base(nodeDir)
	if len(nodeName) != 5 || nodeName[:4] != "node" || nodeName[4] < '0' || nodeName[4] > '5' {
		return "", errors.New("key must belong to one of the six disposable fixture nodes")
	}
	fixtureRoot := filepath.Dir(nodeDir)
	configRaw, err := boundedFile(filepath.Join(fixtureRoot, "application-config.json"), 65536, true)
	if err != nil {
		return "", err
	}
	var config fixtureConfig
	if json.Unmarshal(configRaw, &config) != nil {
		return "", errors.New("invalid fixture application configuration")
	}
	if (config.Profile != "cometbft-lifecycle-local-qualification" && config.Profile != "cometbft-penalty-local-qualification") || config.Lifecycle.Profile != "cometbft-lifecycle-local-qualification" || config.ChainID == "" || config.Lifecycle.ChainID != config.ChainID {
		return "", errors.New("possession signer only accepts the lifecycle local fixture profile")
	}
	raw, err := boundedFile(keyPath, 16384, true)
	if err != nil {
		return "", err
	}
	var key privval.FilePVKey
	if cmtjson.Unmarshal(raw, &key) != nil || key.PrivKey == nil || key.PubKey == nil {
		return "", errors.New("invalid fixture validator key")
	}
	if key.PrivKey.Type() != mldsa65.KeyType || key.PubKey.Type() != mldsa65.KeyType || !bytes.Equal(key.PrivKey.PubKey().Bytes(), key.PubKey.Bytes()) || !bytes.Equal(key.Address, key.PubKey.Address()) {
		return "", errors.New("fixture validator key identity mismatch")
	}
	payload, err := boundedFile(payloadPath, 8192, false)
	if err != nil {
		return "", err
	}
	encoded := base64.StdEncoding.EncodeToString(key.PubKey.Bytes())
	if err := validateProof(payload, config, encoded); err != nil {
		return "", err
	}
	signature, err := key.PrivKey.Sign(payload)
	if err != nil {
		return "", errors.New("fixture possession signing failed")
	}
	return base64.StdEncoding.EncodeToString(signature), nil
}

func run() error {
	key := flag.String("key", "", "absolute disposable fixture validator key path")
	payload := flag.String("payload", "", "bounded domain-bound proof payload file")
	flag.Parse()
	if flag.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	signature, err := signFixtureProof(*key, *payload)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintln(os.Stdout, signature)
	return err
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
