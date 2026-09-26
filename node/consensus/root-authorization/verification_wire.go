package rootauthorization

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
)

// VerificationRequest is untrusted input to the verification-only local helper.
// The caller supplies trusted Policy through a separate process argument.
// Neither input contains private signing material.
type VerificationRequest struct {
	Envelope  Envelope
	Signature []byte
	Artifact  []byte
}

type VerificationResult struct {
	Status              string
	RequestSHA256       string
	ArtifactSHA512      string
	ChainID             string
	Action              Action
	Sequence            uint64
	ProductionQualified bool
}

// DecodeCanonical requires the exact JSON emitted by encoding/json for the
// fixed destination schema. It rejects unknown/duplicate/missing fields, numeric
// aliases, trailing content and alternate base64 representations.
func DecodeCanonical(raw []byte, destination any) error {
	if err := json.Unmarshal(raw, destination); err != nil {
		return err
	}
	canonical, err := json.Marshal(destination)
	if err != nil {
		return err
	}
	if !bytes.Equal(raw, canonical) {
		return ErrEnvelope
	}
	return nil
}

// VerifyRequest performs no action or persistence. Request and artifact limits
// must be enforced by the caller before decoding. A VERIFIED result is local
// process output, not an independent attestation or production acceptance.
func VerifyRequest(policy Policy, raw []byte) (VerificationResult, error) {
	var request VerificationRequest
	if err := DecodeCanonical(raw, &request); err != nil {
		return VerificationResult{}, err
	}
	digest := DigestArtifact(request.Artifact)
	if digest != policy.ExpectedArtifactDigest {
		return VerificationResult{}, ErrPolicy
	}
	if err := Verify(request.Envelope, request.Signature, policy); err != nil {
		return VerificationResult{}, err
	}
	hash := sha256.Sum256(raw)
	return VerificationResult{Status: "VERIFIED", RequestSHA256: hex.EncodeToString(hash[:]), ArtifactSHA512: hex.EncodeToString(digest[:]), ChainID: policy.ChainID, Action: policy.Action, Sequence: policy.ExpectedSequence, ProductionQualified: false}, nil
}
