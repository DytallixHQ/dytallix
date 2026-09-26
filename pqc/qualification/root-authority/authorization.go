// Package rootauthorization checks exceptional root signatures.
// It does not execute an action or authorize a mainnet launch.
package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"crypto/sha512"
	"encoding/binary"
	"errors"

	"github.com/cloudflare/circl/sign/slhdsa"
)

const (
	Profile        = "SLH-DSA-SHAKE-256s"
	Version        = uint16(1)
	PublicKeySize  = 64
	PrivateKeySize = 128
	SignatureSize  = 29792
	MaxChainIDSize = 128
)

type Action string

const (
	Genesis   Action = "genesis"
	Upgrade   Action = "upgrade"
	Emergency Action = "emergency"
)

var (
	ErrEnvelope  = errors.New("invalid root envelope")
	ErrPolicy    = errors.New("root envelope does not match caller policy")
	ErrKey       = errors.New("invalid SLH-DSA-SHAKE-256s root key")
	ErrSignature = errors.New("invalid SLH-DSA-SHAKE-256s root signature")
)

// Envelope commits to an exact artifact and a bounded validity interval.
// Sequence must be nonzero. The caller must persist consumed sequences atomically.
// Heights use the caller's consensus rules. Genesis may use height zero.
type Envelope struct {
	Version         uint16
	Profile         string
	ChainID         string
	Action          Action
	Sequence        uint64
	NotBeforeHeight uint64
	NotAfterHeight  uint64
	ArtifactDigest  [sha512.Size]byte
}

// Policy comes from trusted local configuration and the requested operation.
// Never construct Policy from an untrusted envelope or an embedded signing key.
// ExpectedSequence is the next permitted sequence for this chain and root action.
// Verify does not update replay state. Persist that state with the approved action.
type Policy struct {
	TrustedPublicKey       []byte
	ChainID                string
	Action                 Action
	ExpectedSequence       uint64
	CurrentHeight          uint64
	ExpectedArtifactDigest [sha512.Size]byte
}

// DigestArtifact returns the SHA-512 commitment used in an envelope.
// The caller must define and preserve the artifact's exact byte representation.
func DigestArtifact(artifact []byte) [sha512.Size]byte { return sha512.Sum512(artifact) }

func validAction(action Action) bool {
	return action == Genesis || action == Upgrade || action == Emergency
}

func validChainID(chainID string) bool {
	if len(chainID) == 0 || len(chainID) > MaxChainIDSize {
		return false
	}
	for _, ch := range []byte(chainID) {
		if !((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
			(ch >= '0' && ch <= '9') || ch == '-' || ch == '_' || ch == '.') {
			return false
		}
	}
	return true
}

// signingBytes uses a fixed field order and length-prefixed variable fields.
// Both this encoding and the FIPS 205 context separate the three root actions.
func signingBytes(e Envelope) ([]byte, []byte, error) {
	if e.Version != Version || e.Profile != Profile || !validChainID(e.ChainID) ||
		!validAction(e.Action) || e.Sequence == 0 || e.NotAfterHeight < e.NotBeforeHeight {
		return nil, nil, ErrEnvelope
	}
	var b bytes.Buffer
	b.WriteString("DYTALLIX-ROOT-AUTHORIZATION\x00")
	_ = binary.Write(&b, binary.BigEndian, e.Version)
	for _, value := range []string{e.Profile, e.ChainID, string(e.Action)} {
		_ = binary.Write(&b, binary.BigEndian, uint16(len(value)))
		b.WriteString(value)
	}
	for _, value := range []uint64{e.Sequence, e.NotBeforeHeight, e.NotAfterHeight} {
		_ = binary.Write(&b, binary.BigEndian, value)
	}
	b.Write(e.ArtifactDigest[:])
	return b.Bytes(), []byte("DYTALLIX/ROOT/v1/" + string(e.Action)), nil
}

// Sign signs a root envelope with a caller-supplied SLH-DSA private key.
// It uses OS randomness. It does not generate, save, log, or load a production key.
// The signer must review the artifact and policy before calling Sign.
func Sign(e Envelope, privateKey []byte) ([]byte, error) {
	message, context, err := signingBytes(e)
	if err != nil {
		return nil, err
	}
	if len(privateKey) != PrivateKeySize {
		return nil, ErrKey
	}
	key := slhdsa.PrivateKey{ID: slhdsa.SHAKE_256s}
	if key.UnmarshalBinary(privateKey) != nil {
		return nil, ErrKey
	}
	return slhdsa.SignRandomized(&key, rand.Reader, slhdsa.NewMessage(message), context)
}

// Verify checks one fixed-size signature after all cheap policy and size checks.
// A successful result proves this signature matches the supplied policy.
// It does not execute genesis, upgrades, emergency changes, or launch approval.
// The caller must rate-limit verification and atomically consume ExpectedSequence.
func Verify(e Envelope, signature []byte, policy Policy) error {
	message, context, err := signingBytes(e)
	if err != nil {
		return err
	}
	if !validChainID(policy.ChainID) || !validAction(policy.Action) ||
		policy.ExpectedSequence == 0 || e.ChainID != policy.ChainID ||
		e.Action != policy.Action || e.Sequence != policy.ExpectedSequence ||
		e.ArtifactDigest != policy.ExpectedArtifactDigest ||
		policy.CurrentHeight < e.NotBeforeHeight || policy.CurrentHeight > e.NotAfterHeight {
		return ErrPolicy
	}
	if len(policy.TrustedPublicKey) != PublicKeySize {
		return ErrKey
	}
	if len(signature) != SignatureSize {
		return ErrSignature
	}
	key := slhdsa.PublicKey{ID: slhdsa.SHAKE_256s}
	if key.UnmarshalBinary(policy.TrustedPublicKey) != nil {
		return ErrKey
	}
	if !slhdsa.Verify(&key, slhdsa.NewMessage(message), signature, context) {
		return ErrSignature
	}
	return nil
}
