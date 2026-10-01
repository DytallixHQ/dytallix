package rootauthorization

import (
	"bytes"
	"crypto/sha256"
	"crypto/sha512"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"sort"

	"github.com/cloudflare/circl/sign/slhdsa"
)

// Root genesis 3-of-5 (production activation v1, step A2). Five genesis
// signers, separate from the emergency and upgrade custodians, each sign the
// same genesis envelope over the root bundle. The node verifies every listed
// signature through the pinned helper, needs three, and records the signing
// key IDs in its receipt. These records are public: the signer policy and the
// combined signatures file are distributed identically to every node.

const (
	GenesisRecordSchema = uint16(1)
	GenesisSignerCount  = 5
	GenesisThreshold    = 3
	GenesisSequence     = uint64(1)
	genesisBundleDomain = "DYTALLIX/ROOT-GENESIS/v1\x00"
)

var (
	ErrGenesisPolicy     = errors.New("invalid root genesis signer policy")
	ErrGenesisSignatures = errors.New("invalid root genesis signatures")
)

// AuthorityKey has the node's authority key shape: the key ID is the
// lowercase SHA-256 of the 64 public-key bytes.
type AuthorityKey struct {
	KeyID        string `json:"key_id"`
	PublicKeyHex string `json:"public_key_hex"`
}

// Authority lists keys strictly sorted by key ID.
type Authority struct {
	Keys      []AuthorityKey `json:"keys"`
	Threshold int            `json:"threshold"`
}

// GenesisPolicy is the public genesis signer record for one chain.
type GenesisPolicy struct {
	Schema    uint16    `json:"schema"`
	ChainID   string    `json:"chain_id"`
	Authority Authority `json:"authority"`
}

type GenesisSignature struct {
	KeyID        string `json:"key_id"`
	SignatureHex string `json:"signature_hex"`
}

// GenesisSignatures holds one signer's output, or the combined file the
// nodes read: signatures strictly sorted by key ID.
type GenesisSignatures struct {
	Schema       uint16             `json:"schema"`
	ChainID      string             `json:"chain_id"`
	BundleSHA512 string             `json:"bundle_sha512"`
	Signatures   []GenesisSignature `json:"signatures"`
}

// KeyID returns the lowercase SHA-256 of a public key.
func KeyID(publicKey []byte) string {
	sum := sha256.Sum256(publicKey)
	return hex.EncodeToString(sum[:])
}

// GenesisBundle is the exact root bundle the node signs over: the native
// genesis and configuration bytes, length-prefixed, then the engine genesis
// and release manifest SHA-512 digests.
func GenesisBundle(app, config []byte, engineSHA512, releaseSHA512 [sha512.Size]byte) []byte {
	b := []byte(genesisBundleDomain)
	b = binary.BigEndian.AppendUint64(b, uint64(len(app)))
	b = append(b, app...)
	b = binary.BigEndian.AppendUint64(b, uint64(len(config)))
	b = append(b, config...)
	b = append(b, engineSHA512[:]...)
	return append(b, releaseSHA512[:]...)
}

// GenesisEnvelope is the one envelope every genesis signer signs.
func GenesisEnvelope(chainID string, bundleSHA512 [sha512.Size]byte) Envelope {
	return Envelope{Version: Version, Profile: Profile, ChainID: chainID, Action: Genesis,
		Sequence: GenesisSequence, NotBeforeHeight: 0, NotAfterHeight: 0, ArtifactDigest: bundleSHA512}
}

func genesisPolicyFor(publicKey []byte, chainID string, bundleSHA512 [sha512.Size]byte) Policy {
	return Policy{TrustedPublicKey: publicKey, ChainID: chainID, Action: Genesis,
		ExpectedSequence: GenesisSequence, CurrentHeight: 0, ExpectedArtifactDigest: bundleSHA512}
}

func canonicalHex(value string, size int) ([]byte, bool) {
	raw, err := hex.DecodeString(value)
	if err != nil || len(raw) != size || hex.EncodeToString(raw) != value {
		return nil, false
	}
	return raw, true
}

// Validate checks the fixed 3-of-5 shape, key encodings and key IDs.
func (p GenesisPolicy) Validate() error {
	if p.Schema != GenesisRecordSchema || !validChainID(p.ChainID) ||
		len(p.Authority.Keys) != GenesisSignerCount || p.Authority.Threshold != GenesisThreshold {
		return ErrGenesisPolicy
	}
	previous := ""
	for _, key := range p.Authority.Keys {
		public, ok := canonicalHex(key.PublicKeyHex, PublicKeySize)
		if !ok || ValidatePublicKey(public) != nil || key.KeyID != KeyID(public) || key.KeyID <= previous {
			return ErrGenesisPolicy
		}
		previous = key.KeyID
	}
	return nil
}

func (p GenesisPolicy) key(keyID string) ([]byte, bool) {
	for _, key := range p.Authority.Keys {
		if key.KeyID == keyID {
			public, _ := canonicalHex(key.PublicKeyHex, PublicKeySize)
			return public, true
		}
	}
	return nil, false
}

// SignGenesis signs the genesis envelope with one signer's private key. The
// key must belong to the policy; the signature is checked before return.
func SignGenesis(policy GenesisPolicy, privateKey []byte, bundleSHA512 [sha512.Size]byte) (GenesisSignatures, error) {
	if err := policy.Validate(); err != nil {
		return GenesisSignatures{}, err
	}
	if len(privateKey) != PrivateKeySize {
		return GenesisSignatures{}, ErrKey
	}
	private := slhdsa.PrivateKey{ID: slhdsa.SHAKE_256s}
	if private.UnmarshalBinary(privateKey) != nil {
		return GenesisSignatures{}, ErrKey
	}
	derived, err := private.PublicKey().MarshalBinary()
	if err != nil {
		return GenesisSignatures{}, ErrKey
	}
	keyID := KeyID(derived)
	public, ok := policy.key(keyID)
	if !ok || !bytes.Equal(public, derived) {
		return GenesisSignatures{}, ErrPolicy
	}
	envelope := GenesisEnvelope(policy.ChainID, bundleSHA512)
	signature, err := SignForPolicy(envelope, privateKey, genesisPolicyFor(public, policy.ChainID, bundleSHA512))
	if err != nil {
		return GenesisSignatures{}, err
	}
	return GenesisSignatures{Schema: GenesisRecordSchema, ChainID: policy.ChainID,
		BundleSHA512: hex.EncodeToString(bundleSHA512[:]),
		Signatures:   []GenesisSignature{{KeyID: keyID, SignatureHex: hex.EncodeToString(signature)}}}, nil
}

// VerifyGenesis checks a signatures file against the policy and bundle:
// strictly sorted policy keys, every signature valid, at least the threshold.
// The node repeats these checks and verifies each signature through its
// pinned helper; this check is for signers and the combining operator.
func VerifyGenesis(policy GenesisPolicy, file GenesisSignatures, bundleSHA512 [sha512.Size]byte) error {
	if err := policy.Validate(); err != nil {
		return err
	}
	if file.Schema != GenesisRecordSchema || file.ChainID != policy.ChainID ||
		file.BundleSHA512 != hex.EncodeToString(bundleSHA512[:]) ||
		len(file.Signatures) < policy.Authority.Threshold || len(file.Signatures) > GenesisSignerCount {
		return ErrGenesisSignatures
	}
	envelope := GenesisEnvelope(policy.ChainID, bundleSHA512)
	previous := ""
	for _, entry := range file.Signatures {
		public, ok := policy.key(entry.KeyID)
		signature, encoded := canonicalHex(entry.SignatureHex, SignatureSize)
		if !ok || !encoded || entry.KeyID <= previous {
			return ErrGenesisSignatures
		}
		if err := Verify(envelope, signature, genesisPolicyFor(public, policy.ChainID, bundleSHA512)); err != nil {
			return err
		}
		previous = entry.KeyID
	}
	return nil
}

// CombineGenesis merges signer outputs for the same chain and bundle into
// the one file every node reads, sorted by key ID, and verifies it.
func CombineGenesis(policy GenesisPolicy, parts []GenesisSignatures) (GenesisSignatures, error) {
	if err := policy.Validate(); err != nil {
		return GenesisSignatures{}, err
	}
	if len(parts) == 0 {
		return GenesisSignatures{}, ErrGenesisSignatures
	}
	combined := GenesisSignatures{Schema: GenesisRecordSchema, ChainID: policy.ChainID, BundleSHA512: parts[0].BundleSHA512}
	seen := map[string]bool{}
	for _, part := range parts {
		if part.Schema != GenesisRecordSchema || part.ChainID != combined.ChainID || part.BundleSHA512 != combined.BundleSHA512 {
			return GenesisSignatures{}, ErrGenesisSignatures
		}
		for _, entry := range part.Signatures {
			if seen[entry.KeyID] {
				return GenesisSignatures{}, ErrGenesisSignatures
			}
			seen[entry.KeyID] = true
			combined.Signatures = append(combined.Signatures, entry)
		}
	}
	sort.Slice(combined.Signatures, func(i, j int) bool { return combined.Signatures[i].KeyID < combined.Signatures[j].KeyID })
	digest, ok := canonicalHex(combined.BundleSHA512, sha512.Size)
	if !ok {
		return GenesisSignatures{}, ErrGenesisSignatures
	}
	if err := VerifyGenesis(policy, combined, [sha512.Size]byte(digest)); err != nil {
		return GenesisSignatures{}, err
	}
	return combined, nil
}
