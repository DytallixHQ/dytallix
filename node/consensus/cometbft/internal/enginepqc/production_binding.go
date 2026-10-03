package enginepqc

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"

	"github.com/cometbft/cometbft/types"
)

// Node roles in a production network (production activation v1, design F).
const (
	RoleValidator = "validator"
	RoleSentry    = "sentry"
	RoleEndpoint  = "endpoint"
)

// ProductionBinding is one host's public record in the published pin plan
// (production activation v1, A4): its role, the exact configuration, genesis
// and transport files, and its peer and validator public keys, by SHA-256.
// It holds no private key. The supervisor and the engine check it at start;
// a change is a reviewed configuration update and a restart.
type ProductionBinding struct {
	Schema                   uint16 `json:"schema"`
	Role                     string `json:"role"`
	ChainID                  string `json:"chain_id"`
	ConfigSHA256             string `json:"config_sha256"`
	GenesisSHA256            string `json:"genesis_sha256"`
	TransportSHA256          string `json:"transport_sha256"`
	PeerPublicKeySHA256      string `json:"peer_public_key_sha256"`
	ValidatorPublicKeySHA256 string `json:"validator_public_key_sha256"`
}

// DecodeProductionBinding reads a binding in canonical compact JSON.
func DecodeProductionBinding(raw []byte) (ProductionBinding, error) {
	var binding ProductionBinding
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&binding); err != nil {
		return ProductionBinding{}, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return ProductionBinding{}, errors.New("trailing production binding data")
	}
	canonical, err := json.Marshal(binding)
	if err != nil || !bytes.Equal(canonical, bytes.TrimSpace(raw)) {
		return ProductionBinding{}, errors.New("production binding must use canonical compact JSON")
	}
	return binding, nil
}

// LoadProductionBinding reads a bounded binding file.
func LoadProductionBinding(path string) (ProductionBinding, error) {
	raw, err := privateFile(path, 4096)
	if err != nil {
		return ProductionBinding{}, err
	}
	return DecodeProductionBinding(raw)
}

func digestHex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// NewProductionBinding is the binding of a loaded production runtime in a
// role: the record the operator publishes in the pin plan. It is checked as
// the engine checks it at start, so a genesis validator key never binds as a
// sentry or endpoint.
func NewProductionBinding(runtime *Runtime, role string) (ProductionBinding, error) {
	if runtime == nil || runtime.Transport.Profile != ProductionProfile {
		return ProductionBinding{}, errors.New("a production binding applies only to the production transport profile")
	}
	binding := ProductionBinding{
		Schema:                   1,
		Role:                     role,
		ChainID:                  runtime.Genesis.ChainID,
		ConfigSHA256:             hex.EncodeToString(runtime.configSHA256[:]),
		GenesisSHA256:            hex.EncodeToString(runtime.genesisSHA256[:]),
		TransportSHA256:          hex.EncodeToString(runtime.transportSHA256[:]),
		PeerPublicKeySHA256:      digestHex(runtime.NodeKey.PubKey().Bytes()),
		ValidatorPublicKeySHA256: digestHex(runtime.Validator.Key.PubKey.Bytes()),
	}
	if err := ValidateProductionBinding(runtime, binding); err != nil {
		return ProductionBinding{}, err
	}
	return binding, nil
}

// ValidateProductionBinding checks a loaded production runtime against its
// binding: the same file bytes the loader parsed, the loaded peer and
// validator keys, and the role. Only a validator's key is in the genesis
// validator set; a sentry or endpoint carries a key the genesis set lacks,
// so it can never sign for the chain.
func ValidateProductionBinding(runtime *Runtime, binding ProductionBinding) error {
	if runtime == nil || runtime.Transport.Profile != ProductionProfile {
		return errors.New("a production binding applies only to the production transport profile")
	}
	canonical := func(value string) bool {
		decoded, err := hex.DecodeString(value)
		return err == nil && len(decoded) == sha256.Size && hex.EncodeToString(decoded) == value
	}
	for _, value := range []string{binding.ConfigSHA256, binding.GenesisSHA256, binding.TransportSHA256, binding.PeerPublicKeySHA256, binding.ValidatorPublicKeySHA256} {
		if !canonical(value) {
			return errors.New("production binding digests must be lowercase SHA-256")
		}
	}
	if binding.Schema != 1 || (binding.Role != RoleValidator && binding.Role != RoleSentry && binding.Role != RoleEndpoint) {
		return errors.New("production binding schema or role is unsupported")
	}
	if runtime.Genesis.ChainID != binding.ChainID ||
		hex.EncodeToString(runtime.configSHA256[:]) != binding.ConfigSHA256 ||
		hex.EncodeToString(runtime.genesisSHA256[:]) != binding.GenesisSHA256 ||
		hex.EncodeToString(runtime.transportSHA256[:]) != binding.TransportSHA256 {
		return errors.New("engine files differ from the production binding")
	}
	if digestHex(runtime.NodeKey.PubKey().Bytes()) != binding.PeerPublicKeySHA256 {
		return errors.New("peer key differs from the production binding")
	}
	validator := runtime.Validator.Key.PubKey
	if digestHex(validator.Bytes()) != binding.ValidatorPublicKeySHA256 {
		return errors.New("validator key differs from the production binding")
	}
	return roleMatchesGenesis(runtime.Genesis, validator.Bytes(), binding.Role)
}

// roleMatchesGenesis refuses a sentry or endpoint whose validator key is in
// the genesis validator set, and a validator whose key is not.
func roleMatchesGenesis(genesis *types.GenesisDoc, validator []byte, role string) error {
	inGenesis := false
	for _, member := range genesis.Validators {
		if bytes.Equal(member.PubKey.Bytes(), validator) {
			inGenesis = true
		}
	}
	if inGenesis != (role == RoleValidator) {
		return errors.New("only a validator's key may be in the genesis validator set")
	}
	return nil
}
