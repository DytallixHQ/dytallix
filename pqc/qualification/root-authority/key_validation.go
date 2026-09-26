package rootauthorization

import (
	"bytes"
	"crypto/rand"

	"github.com/cloudflare/circl/sign/slhdsa"
)

// ValidatePublicKey checks the fixed-profile public-key encoding.
// A correctly sized SLH-DSA key has no algebraic validity test. This check does
// not prove ownership, approved custody, a trusted origin, or a valid key pair.
func ValidatePublicKey(publicKey []byte) error {
	if len(publicKey) != PublicKeySize {
		return ErrKey
	}
	key := slhdsa.PublicKey{ID: slhdsa.SHAKE_256s}
	if err := key.UnmarshalBinary(publicKey); err != nil {
		return ErrKey
	}
	encoded, err := key.MarshalBinary()
	if err != nil || !bytes.Equal(encoded, publicKey) {
		return ErrKey
	}
	return nil
}

// CheckKeyPair checks a private key against an independently trusted public key.
// The check signs a public-key challenge under a separate validation context.
// It does not produce a root action signature or establish a custody policy.
// It checks the signing path because decoding alone cannot detect inconsistent
// private seeds and an embedded public root. No key bytes leave this function.
func CheckKeyPair(privateKey, trustedPublicKey []byte) error {
	if err := ValidatePublicKey(trustedPublicKey); err != nil {
		return err
	}
	if len(privateKey) != PrivateKeySize {
		return ErrKey
	}
	private := slhdsa.PrivateKey{ID: slhdsa.SHAKE_256s}
	if private.UnmarshalBinary(privateKey) != nil {
		return ErrKey
	}
	public := private.PublicKey()
	encoded, err := public.MarshalBinary()
	if err != nil || !bytes.Equal(encoded, trustedPublicKey) {
		return ErrKey
	}
	context := []byte("DYTALLIX/ROOT/KEY-VALIDATION/v1")
	message := slhdsa.NewMessage(trustedPublicKey)
	proof, err := slhdsa.SignRandomized(&private, rand.Reader, message, context)
	if err != nil {
		return err
	}
	if !slhdsa.Verify(&public, message, proof, context) {
		return ErrKey
	}
	return nil
}

// SignForPolicy checks trusted caller policy before accessing the signing path.
// The caller must approve policy independently of the submitted envelope.
// This helper cannot establish human approval or persistent replay protection.
func SignForPolicy(e Envelope, privateKey []byte, policy Policy) ([]byte, error) {
	if _, _, err := signingBytes(e); err != nil {
		return nil, err
	}
	if !validChainID(policy.ChainID) || !validAction(policy.Action) ||
		policy.ExpectedSequence == 0 || e.ChainID != policy.ChainID ||
		e.Action != policy.Action || e.Sequence != policy.ExpectedSequence ||
		e.ArtifactDigest != policy.ExpectedArtifactDigest ||
		policy.CurrentHeight < e.NotBeforeHeight || policy.CurrentHeight > e.NotAfterHeight {
		return nil, ErrPolicy
	}
	if err := CheckKeyPair(privateKey, policy.TrustedPublicKey); err != nil {
		return nil, err
	}
	signature, err := Sign(e, privateKey)
	if err != nil {
		return nil, err
	}
	if err := Verify(e, signature, policy); err != nil {
		return nil, err
	}
	return signature, nil
}
