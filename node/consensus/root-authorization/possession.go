package rootauthorization

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"

	"github.com/cloudflare/circl/sign/slhdsa"
)

// A proof of possession is the public evidence the custody intakes ask for:
// one SLH-DSA signature by the key over a challenge that binds the chain,
// the controller, the purpose, the authority epoch, the key and the ceremony
// session. Its FIPS 205 context is its own, so a proof is never a valid root
// action, root genesis or key validation signature.
const (
	PossessionSchema  = "dytallix.key-possession.v1"
	possessionContext = "DYTALLIX/KEY-POSSESSION/v1"
)

var (
	ErrPossession = errors.New("invalid proof of possession")
	labelPattern  = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)
)

// PossessionChallenge is what a proof signs, as canonical JSON in this
// field order. AuthorityEpoch is null for the genesis signers, which have
// no epoch, and positive for the other purposes.
type PossessionChallenge struct {
	Schema         string  `json:"schema"`
	ChainID        string  `json:"chain_id"`
	ControllerID   string  `json:"controller_id"`
	Purpose        string  `json:"purpose"`
	AuthorityEpoch *uint64 `json:"authority_epoch"`
	KeyID          string  `json:"key_id"`
	Session        string  `json:"session"`
}

// PossessionProof is the public record: the challenge, the public key and
// the signature.
type PossessionProof struct {
	Challenge    PossessionChallenge `json:"challenge"`
	PublicKeyHex string              `json:"public_key_hex"`
	SignatureHex string              `json:"signature_hex"`
}

func (c PossessionChallenge) validate() error {
	if c.Schema != PossessionSchema || !kitPurpose(c.Purpose) || !labelPattern.MatchString(c.ChainID) ||
		!labelPattern.MatchString(c.ControllerID) || !labelPattern.MatchString(c.Session) {
		return ErrPossession
	}
	if (c.Purpose == "genesis") != (c.AuthorityEpoch == nil) || (c.AuthorityEpoch != nil && *c.AuthorityEpoch == 0) {
		return ErrPossession
	}
	if _, ok := canonicalHex(c.KeyID, 32); !ok {
		return ErrPossession
	}
	return nil
}

// ProvePossession signs the challenge with a private key whose public key
// has the challenge's key ID.
func ProvePossession(challenge PossessionChallenge, privateKey []byte) (PossessionProof, error) {
	if err := challenge.validate(); err != nil {
		return PossessionProof{}, err
	}
	if len(privateKey) != PrivateKeySize {
		return PossessionProof{}, ErrKey
	}
	private := slhdsa.PrivateKey{ID: slhdsa.SHAKE_256s}
	if err := private.UnmarshalBinary(privateKey); err != nil {
		return PossessionProof{}, ErrKey
	}
	public := private.PublicKey()
	publicBytes, err := public.MarshalBinary()
	if err != nil {
		return PossessionProof{}, err
	}
	if KeyID(publicBytes) != challenge.KeyID {
		return PossessionProof{}, errors.New("the private key does not match the challenge's key ID")
	}
	message, err := json.Marshal(challenge)
	if err != nil {
		return PossessionProof{}, err
	}
	signature, err := slhdsa.SignRandomized(&private, rand.Reader, slhdsa.NewMessage(message), []byte(possessionContext))
	if err != nil {
		return PossessionProof{}, err
	}
	proof := PossessionProof{Challenge: challenge, PublicKeyHex: hex.EncodeToString(publicBytes), SignatureHex: hex.EncodeToString(signature)}
	// Verifying the result also catches private seeds inconsistent with the
	// embedded public key, which decoding alone cannot.
	if err := VerifyPossession(proof); err != nil {
		return PossessionProof{}, ErrKey
	}
	return proof, nil
}

// VerifyPossession checks a proof on its own: the challenge, that the public
// key has its key ID, and the signature.
func VerifyPossession(proof PossessionProof) error {
	if err := proof.Challenge.validate(); err != nil {
		return err
	}
	publicBytes, ok := canonicalHex(proof.PublicKeyHex, PublicKeySize)
	if !ok || ValidatePublicKey(publicBytes) != nil || KeyID(publicBytes) != proof.Challenge.KeyID {
		return ErrPossession
	}
	signature, ok := canonicalHex(proof.SignatureHex, SignatureSize)
	if !ok {
		return ErrPossession
	}
	public := slhdsa.PublicKey{ID: slhdsa.SHAKE_256s}
	if public.UnmarshalBinary(publicBytes) != nil {
		return ErrPossession
	}
	message, err := json.Marshal(proof.Challenge)
	if err != nil {
		return err
	}
	if !slhdsa.Verify(&public, slhdsa.NewMessage(message), signature, []byte(possessionContext)) {
		return ErrPossession
	}
	return nil
}
