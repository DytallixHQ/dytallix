package enginepqc

import (
	"encoding/hex"
	"errors"
	"strings"

	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
)

// ProductionCandidateBinding is supplied by a separately approved release
// record. It contains only public identifiers and hashes, never private keys.
type ProductionCandidateBinding struct {
	ChainID         string
	ConfigSHA256    string
	GenesisSHA256   string
	TransportSHA256 string
}

// ValidateProductionCandidateBinding checks the same bytes that the loader
// parsed. A caller must verify the authority of the binding separately. This
// function does not start the engine or authorize production activation.
func ValidateProductionCandidateBinding(home string, binding ProductionCandidateBinding) error {
	validDigest := func(value string) bool {
		if len(value) != 64 {
			return false
		}
		decoded, err := hex.DecodeString(value)
		return err == nil && hex.EncodeToString(decoded) == value
	}
	if binding.ChainID == "" || len(binding.ChainID) > 64 || strings.TrimSpace(binding.ChainID) != binding.ChainID ||
		!validDigest(binding.ConfigSHA256) || !validDigest(binding.GenesisSHA256) || !validDigest(binding.TransportSHA256) {
		return errors.New("complete canonical production candidate binding required")
	}
	runtime, err := load(home, ProductionCandidateProfile, true)
	if err != nil {
		return err
	}
	if runtime.Genesis.ChainID != binding.ChainID ||
		hex.EncodeToString(runtime.configSHA256[:]) != binding.ConfigSHA256 ||
		hex.EncodeToString(runtime.genesisSHA256[:]) != binding.GenesisSHA256 ||
		hex.EncodeToString(runtime.transportSHA256[:]) != binding.TransportSHA256 {
		return errors.New("candidate files differ from approved chain and configuration binding")
	}
	return nil
}

// ValidateProductionTransportCandidate checks a proposed transport config
// without loading keys, opening a socket, or authorizing production startup.
// The caller must supply the exact chain ID read from its proposed genesis.
func ValidateProductionTransportCandidate(c *cfg.Config, tc TransportConfig, key *p2p.NodeKey, chainID string) error {
	if BuildProfile != "dytallix_pqc_only" || RPCBuildProfile != "dytallix-pqc-unix-v1" {
		return errors.New("production candidate requires the selected PQC-only IPC build")
	}
	if chainID == "" || len(chainID) > 64 || strings.TrimSpace(chainID) != chainID || key == nil || key.PrivKey == nil || key.PrivKey.Type() != mldsa65.KeyType {
		return errors.New("production candidate requires an exact chain ID and ML-DSA-65 peer key")
	}
	if err := validateIsolationForProfile(c, ProductionCandidateProfile); err != nil {
		return err
	}
	_, _, _, err := validatePinsForProfile(tc, c, key, chainID, ProductionCandidateProfile)
	return err
}
