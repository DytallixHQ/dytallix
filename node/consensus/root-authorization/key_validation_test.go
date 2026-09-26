package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"errors"
	"fmt"
	"math"
	"testing"

	"github.com/cloudflare/circl/sign/slhdsa"
)

func localPair(t *testing.T) ([]byte, []byte) {
	t.Helper()
	public, private, err := slhdsa.GenerateKey(rand.Reader, slhdsa.SHAKE_256s)
	if err != nil {
		t.Fatal(err)
	}
	pub, err := public.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	priv, err := private.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	return pub, priv
}

func localRequest(pub []byte) (Envelope, Policy) {
	e := Envelope{Version: Version, Profile: Profile, ChainID: "local-qualification", Action: Upgrade,
		Sequence: 7, NotBeforeHeight: 10, NotAfterHeight: 20, ArtifactDigest: DigestArtifact([]byte("local fixture only"))}
	return e, Policy{TrustedPublicKey: pub, ChainID: e.ChainID, Action: e.Action,
		ExpectedSequence: e.Sequence, CurrentHeight: 15, ExpectedArtifactDigest: e.ArtifactDigest}
}

func TestPublicKeyEncoding(t *testing.T) {
	for _, size := range []int{0, 1, 32, 63, 65, 128, 1952} {
		t.Run(fmt.Sprintf("reject-size-%d", size), func(t *testing.T) {
			if err := ValidatePublicKey(make([]byte, size)); !errors.Is(err, ErrKey) {
				t.Fatalf("got %v", err)
			}
		})
	}
	// SLH-DSA public keys are two byte strings. Encoding validity is not trust.
	for _, fill := range []byte{0, 1, 255} {
		t.Run(fmt.Sprintf("structural-only-%d", fill), func(t *testing.T) {
			if err := ValidatePublicKey(bytes.Repeat([]byte{fill}, PublicKeySize)); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestKeyPairConsistency(t *testing.T) {
	pub, priv := localPair(t)
	if err := CheckKeyPair(priv, pub); err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name            string
		private, public []byte
	}{
		{"private-missing", nil, pub}, {"private-short", priv[:127], pub},
		{"private-long", append(bytes.Clone(priv), 0), pub}, {"public-short", priv, pub[:63]},
		{"wrong-public", priv, bytes.Repeat([]byte{1}, PublicKeySize)},
		{"zero-pair", make([]byte, PrivateKeySize), make([]byte, PublicKeySize)},
	}
	for _, offset := range []int{0, 64, 96} {
		changed := bytes.Clone(priv)
		changed[offset] ^= 1
		cases = append(cases, struct {
			name            string
			private, public []byte
		}{fmt.Sprintf("corrupt-private-component-%d", offset), changed, pub})
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if err := CheckKeyPair(tc.private, tc.public); !errors.Is(err, ErrKey) {
				t.Fatalf("got %v", err)
			}
		})
	}
}

func TestSigningPolicyRejectsBeforePrivateKeyAccess(t *testing.T) {
	e, p := localRequest(make([]byte, PublicKeySize))
	cases := []struct {
		name   string
		mutate func(*Envelope, *Policy)
		want   error
	}{
		{"malformed-envelope", func(e *Envelope, p *Policy) { e.Version++ }, ErrEnvelope},
		{"empty-policy-chain", func(e *Envelope, p *Policy) { p.ChainID = "" }, ErrPolicy},
		{"policy-chain", func(e *Envelope, p *Policy) { p.ChainID = "other" }, ErrPolicy},
		{"policy-routine-action", func(e *Envelope, p *Policy) { p.Action = "transaction" }, ErrPolicy},
		{"policy-action", func(e *Envelope, p *Policy) { p.Action = Genesis }, ErrPolicy},
		{"policy-zero-sequence", func(e *Envelope, p *Policy) { p.ExpectedSequence = 0 }, ErrPolicy},
		{"policy-sequence", func(e *Envelope, p *Policy) { p.ExpectedSequence++ }, ErrPolicy},
		{"policy-artifact", func(e *Envelope, p *Policy) { p.ExpectedArtifactDigest[0] ^= 1 }, ErrPolicy},
		{"before-validity", func(e *Envelope, p *Policy) { p.CurrentHeight = 9 }, ErrPolicy},
		{"after-validity", func(e *Envelope, p *Policy) { p.CurrentHeight = 21 }, ErrPolicy},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			copyE, copyP := e, p
			tc.mutate(&copyE, &copyP)
			sig, err := SignForPolicy(copyE, nil, copyP)
			if sig != nil || !errors.Is(err, tc.want) {
				t.Fatalf("signature length %d; error %v", len(sig), err)
			}
		})
	}
}

func TestCheckedSigningAndKeyMismatch(t *testing.T) {
	pub, priv := localPair(t)
	for _, action := range []Action{Genesis, Upgrade, Emergency} {
		t.Run(string(action), func(t *testing.T) {
			e, p := localRequest(pub)
			e.Action = action
			p.Action = action
			sig, err := SignForPolicy(e, priv, p)
			if err != nil {
				t.Fatal(err)
			}
			if err := Verify(e, sig, p); err != nil {
				t.Fatal(err)
			}
			p.ExpectedSequence++
			if err := Verify(e, sig, p); !errors.Is(err, ErrPolicy) {
				t.Fatalf("replay: %v", err)
			}
		})
	}
	e, p := localRequest(pub)
	if sig, err := SignForPolicy(e, make([]byte, PrivateKeySize), p); sig != nil || !errors.Is(err, ErrKey) {
		t.Fatalf("got %v", err)
	}
}

func TestSignatureBindsEachValidityBoundary(t *testing.T) {
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	sig, err := Sign(e, priv)
	if err != nil {
		t.Fatal(err)
	}
	for _, boundary := range []string{"lower", "upper"} {
		t.Run(boundary, func(t *testing.T) {
			changed := e
			if boundary == "lower" {
				changed.NotBeforeHeight++
			} else {
				changed.NotAfterHeight++
			}
			if err := Verify(changed, sig, p); !errors.Is(err, ErrSignature) {
				t.Fatalf("got %v", err)
			}
		})
	}
}

func TestMaximumSequenceDoesNotImplicitlyWrap(t *testing.T) {
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	e.Sequence = math.MaxUint64
	p.ExpectedSequence = e.Sequence
	sig, err := Sign(e, priv)
	if err != nil {
		t.Fatal(err)
	}
	if err := Verify(e, sig, p); err != nil {
		t.Fatal(err)
	}
	p.ExpectedSequence++
	if err := Verify(e, sig, p); !errors.Is(err, ErrPolicy) {
		t.Fatalf("wrapped policy accepted: %v", err)
	}
	// The production state machine still needs an explicit sequence-exhaustion rule.
}
