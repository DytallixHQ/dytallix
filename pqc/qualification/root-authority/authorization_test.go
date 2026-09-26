package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"errors"
	"strings"
	"testing"

	"github.com/cloudflare/circl/sign/slhdsa"
)

func TestExceptionalAuthorization(t *testing.T) {
	pub, priv, err := slhdsa.GenerateKey(rand.Reader, slhdsa.SHAKE_256s)
	if err != nil {
		t.Fatal(err)
	}
	publicKey, err := pub.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	privateKey, err := priv.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	if len(publicKey) != PublicKeySize || len(privateKey) != PrivateKeySize {
		t.Fatal("unexpected FIPS 205 key sizes")
	}
	for _, action := range []Action{Genesis, Upgrade, Emergency} {
		t.Run(string(action), func(t *testing.T) {
			envelope := Envelope{Version: Version, Profile: Profile, ChainID: "local-root-test", Action: action,
				Sequence: 1, NotBeforeHeight: 10, NotAfterHeight: 20, ArtifactDigest: DigestArtifact([]byte("local test artifact"))}
			policy := Policy{TrustedPublicKey: publicKey, ChainID: envelope.ChainID, Action: action,
				ExpectedSequence: 1, CurrentHeight: 10, ExpectedArtifactDigest: envelope.ArtifactDigest}
			signature, err := Sign(envelope, privateKey)
			if err != nil {
				t.Fatal(err)
			}
			if len(signature) != SignatureSize {
				t.Fatal("unexpected FIPS 205 signature size")
			}
			if err := Verify(envelope, signature, policy); err != nil {
				t.Fatal(err)
			}
			policy.CurrentHeight = 20
			if err := Verify(envelope, signature, policy); err != nil {
				t.Fatalf("inclusive upper height: %v", err)
			}

			cases := []struct {
				name   string
				mutate func(*Envelope, *[]byte, *Policy)
				want   error
			}{
				{"tampered signature", func(e *Envelope, s *[]byte, p *Policy) { (*s)[100] ^= 1 }, ErrSignature},
				{"missing signature", func(e *Envelope, s *[]byte, p *Policy) { *s = nil }, ErrSignature},
				{"short signature", func(e *Envelope, s *[]byte, p *Policy) { *s = (*s)[:SignatureSize-1] }, ErrSignature},
				{"long signature", func(e *Envelope, s *[]byte, p *Policy) { *s = append(*s, 0) }, ErrSignature},
				{"missing trusted key", func(e *Envelope, s *[]byte, p *Policy) { p.TrustedPublicKey = nil }, ErrKey},
				{"wrong trusted key", func(e *Envelope, s *[]byte, p *Policy) { p.TrustedPublicKey = bytes.Repeat([]byte{1}, PublicKeySize) }, ErrSignature},
				{"operational key size", func(e *Envelope, s *[]byte, p *Policy) { p.TrustedPublicKey = make([]byte, 1952) }, ErrKey},
				{"wrong policy chain", func(e *Envelope, s *[]byte, p *Policy) { p.ChainID = "other-chain" }, ErrPolicy},
				{"replayed sequence", func(e *Envelope, s *[]byte, p *Policy) { p.ExpectedSequence = 2 }, ErrPolicy},
				{"zero policy sequence", func(e *Envelope, s *[]byte, p *Policy) { p.ExpectedSequence = 0 }, ErrPolicy},
				{"before height", func(e *Envelope, s *[]byte, p *Policy) { p.CurrentHeight = 9 }, ErrPolicy},
				{"expired height", func(e *Envelope, s *[]byte, p *Policy) { p.CurrentHeight = 21 }, ErrPolicy},
				{"wrong expected artifact", func(e *Envelope, s *[]byte, p *Policy) { p.ExpectedArtifactDigest[0] ^= 1 }, ErrPolicy},
				{"changed artifact", func(e *Envelope, s *[]byte, p *Policy) {
					e.ArtifactDigest[0] ^= 1
					p.ExpectedArtifactDigest = e.ArtifactDigest
				}, ErrSignature},
				{"changed chain", func(e *Envelope, s *[]byte, p *Policy) { e.ChainID = "other-chain"; p.ChainID = e.ChainID }, ErrSignature},
				{"changed sequence", func(e *Envelope, s *[]byte, p *Policy) { e.Sequence = 2; p.ExpectedSequence = 2 }, ErrSignature},
				{"changed interval", func(e *Envelope, s *[]byte, p *Policy) { e.NotAfterHeight++ }, ErrSignature},
				{"changed action", func(e *Envelope, s *[]byte, p *Policy) {
					e.Action = Upgrade
					if action == Upgrade {
						e.Action = Genesis
					}
					p.Action = e.Action
				}, ErrSignature},
				{"routine action", func(e *Envelope, s *[]byte, p *Policy) { e.Action = "transaction" }, ErrEnvelope},
				{"wrong version", func(e *Envelope, s *[]byte, p *Policy) { e.Version++ }, ErrEnvelope},
				{"legacy sphincs label", func(e *Envelope, s *[]byte, p *Policy) { e.Profile = "SPHINCS+-SHAKE-256s-simple" }, ErrEnvelope},
				{"wrong profile", func(e *Envelope, s *[]byte, p *Policy) { e.Profile = "SLH-DSA-SHAKE-192s" }, ErrEnvelope},
				{"oversized chain", func(e *Envelope, s *[]byte, p *Policy) { e.ChainID = strings.Repeat("a", MaxChainIDSize+1) }, ErrEnvelope},
				{"empty chain", func(e *Envelope, s *[]byte, p *Policy) { e.ChainID = "" }, ErrEnvelope},
				{"ambiguous chain", func(e *Envelope, s *[]byte, p *Policy) { e.ChainID = "chain\x00suffix" }, ErrEnvelope},
				{"inverted interval", func(e *Envelope, s *[]byte, p *Policy) { e.NotBeforeHeight = 21 }, ErrEnvelope},
				{"zero envelope sequence", func(e *Envelope, s *[]byte, p *Policy) { e.Sequence = 0 }, ErrEnvelope},
			}
			for _, tc := range cases {
				t.Run(tc.name, func(t *testing.T) {
					e, p, s := envelope, policy, bytes.Clone(signature)
					tc.mutate(&e, &s, &p)
					if err := Verify(e, s, p); !errors.Is(err, tc.want) {
						t.Fatalf("got %v; want %v", err, tc.want)
					}
				})
			}
			message, _, err := signingBytes(envelope)
			if err != nil {
				t.Fatal(err)
			}
			if slhdsa.Verify(&pub, slhdsa.NewMessage(message), signature, nil) {
				t.Fatal("signature accepted without root context")
			}
			if _, err := Sign(envelope, make([]byte, 32)); !errors.Is(err, ErrKey) {
				t.Fatal("accepted classical private key length")
			}
		})
	}
}

func TestMalformedEnvelopeBeforeSigning(t *testing.T) {
	if _, err := Sign(Envelope{}, nil); !errors.Is(err, ErrEnvelope) {
		t.Fatalf("got %v", err)
	}
	if err := Verify(Envelope{}, nil, Policy{}); !errors.Is(err, ErrEnvelope) {
		t.Fatalf("got %v", err)
	}
}
