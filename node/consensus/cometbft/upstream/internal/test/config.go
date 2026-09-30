package test

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"

	"github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmtos "github.com/cometbft/cometbft/libs/os"
)

func ResetTestRoot(testName string) *config.Config {
	return ResetTestRootWithChainID(testName, "")
}

func ResetTestRootWithChainID(testName string, chainID string) *config.Config {
	// create a unique, concurrency-safe test directory under os.TempDir()
	rootDir, err := os.MkdirTemp("", fmt.Sprintf("%s-%s_", chainID, testName))
	if err != nil {
		panic(err)
	}

	config.EnsureRoot(rootDir)

	baseConfig := config.DefaultBaseConfig()
	genesisFilePath := filepath.Join(rootDir, baseConfig.Genesis)
	privKeyFilePath := filepath.Join(rootDir, baseConfig.PrivValidatorKey)
	privStateFilePath := filepath.Join(rootDir, baseConfig.PrivValidatorState)

	if !cmtos.FileExists(genesisFilePath) {
		if chainID == "" {
			chainID = DefaultTestChainID
		}
		testGenesis := fmt.Sprintf(testGenesisFmt, chainID, testValidatorPubKey)
		cmtos.MustWriteFile(genesisFilePath, []byte(testGenesis), 0o644)
	}
	// we always overwrite the priv val
	cmtos.MustWriteFile(privKeyFilePath, []byte(testPrivValidatorKey), 0o644)
	cmtos.MustWriteFile(privStateFilePath, []byte(testPrivValidatorState), 0o644)

	config := config.TestConfig().SetRoot(rootDir)
	return config
}

var testGenesisFmt = `{
  "genesis_time": "2018-10-10T08:20:13.695936996Z",
  "chain_id": "%s",
  "initial_height": "1",
	"consensus_params": {
		"block": {
			"max_bytes": "22020096",
			"max_gas": "-1",
			"time_iota_ms": "10"
		},
		"evidence": {
			"max_age_num_blocks": "100000",
			"max_age_duration": "172800000000000",
			"max_bytes": "1048576"
		},
		"validator": {
			"pub_key_types": [
				"ml_dsa_65"
			]
		},
		"abci": {
			"vote_extensions_enable_height": "0"
		},
		"version": {}
	},
  "validators": [
    {
      "pub_key": %s,
      "power": "10",
      "name": ""
    }
  ],
  "app_hash": ""
}`

// The test validator key is ML-DSA-65, derived from a fixed seed so that
// every test root holds the same validator.
var testPrivValidatorKey, testValidatorPubKey = testValidatorKeyFiles()

func testValidatorKeyFiles() (string, string) {
	privKey, err := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{0x01}, mldsa65.SeedSize))
	if err != nil {
		panic(err)
	}
	pubKey := privKey.PubKey()
	key, err := cmtjson.MarshalIndent(struct {
		Address crypto.Address `json:"address"`
		PubKey  crypto.PubKey  `json:"pub_key"`
		PrivKey crypto.PrivKey `json:"priv_key"`
	}{pubKey.Address(), pubKey, privKey}, "", "  ")
	if err != nil {
		panic(err)
	}
	genesisKey, err := cmtjson.Marshal(pubKey)
	if err != nil {
		panic(err)
	}
	return string(key), string(genesisKey)
}

var testPrivValidatorState = `{
  "height": "0",
  "round": 0,
  "step": 0
}`
