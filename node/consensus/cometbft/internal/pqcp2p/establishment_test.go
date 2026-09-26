package pqcp2p

import (
	"bytes"
	"errors"
	"testing"
)

func identity(t *testing.T) *Identity {
	t.Helper()
	k, e := GenerateIdentity()
	if e != nil {
		t.Fatal(e)
	}
	return k
}
func start(t *testing.T) (*Initiator, *Responder, Offer) {
	t.Helper()
	a, b := identity(t), identity(t)
	i, h, e := NewInitiator("fixture-chain", a, b.PublicKey())
	if e != nil {
		t.Fatal(e)
	}
	s, o, e := NewResponder("fixture-chain", b, a.PublicKey(), h)
	if e != nil {
		t.Fatal(e)
	}
	return i, s, o
}
func TestMutualAuthenticationAndKeyConfirmation(t *testing.T) {
	i, s, o := start(t)
	pi, r, e := i.Respond(o)
	if e != nil {
		t.Fatal(e)
	}
	ps, server, e := s.Confirm(r)
	if e != nil {
		t.Fatal(e)
	}
	a, client, e := pi.Finish(server)
	if e != nil {
		t.Fatal(e)
	}
	b, e := ps.Finish(client)
	if e != nil {
		t.Fatal(e)
	}
	if a.Send != b.Receive || a.Receive != b.Send || a.Send == a.Receive || a.Send == [32]byte{} {
		t.Fatal("directional key mismatch")
	}
	if _, _, e = i.Respond(o); !errors.Is(e, ErrConsumed) {
		t.Fatal("initiator replay accepted")
	}
	if _, _, e = s.Confirm(r); !errors.Is(e, ErrConsumed) {
		t.Fatal("responder replay accepted")
	}
	if _, _, e = pi.Finish(server); !errors.Is(e, ErrConsumed) {
		t.Fatal("initiator confirmation replay accepted")
	}
	if _, e = ps.Finish(client); !errors.Is(e, ErrConsumed) {
		t.Fatal("responder confirmation replay accepted")
	}
}
func TestRejectWrongNetworkPinsAndSelf(t *testing.T) {
	a, b, c := identity(t), identity(t), identity(t)
	_, h, e := NewInitiator("network-a", a, b.PublicKey())
	if e != nil {
		t.Fatal(e)
	}
	cases := []struct {
		name, network string
		local         *Identity
		pin           []byte
	}{
		{"wrong network", "network-b", b, a.PublicKey()},
		{"wrong initiator pin", "network-a", b, c.PublicKey()},
		{"wrong responder", "network-a", c, a.PublicKey()},
		{"short pin", "network-a", b, []byte("ed25519")},
	}
	for _, tt := range cases {
		t.Run(tt.name, func(t *testing.T) {
			if _, _, e := NewResponder(tt.network, tt.local, tt.pin, h); !errors.Is(e, ErrRejected) {
				t.Fatal("accepted unauthorized configuration")
			}
		})
	}
	if _, _, e := NewInitiator("network-a", a, a.PublicKey()); e == nil {
		t.Fatal("accepted identical roles")
	}
	if _, _, e := NewInitiator("", a, b.PublicKey()); e == nil {
		t.Fatal("accepted empty network")
	}
	if _, _, e := NewInitiator("network-a", a, make([]byte, 32)); e == nil {
		t.Fatal("accepted classical-size identity")
	}
}
func TestOfferTamperingConsumesAttempt(t *testing.T) {
	for _, field := range []string{"kem", "signature", "length"} {
		t.Run(field, func(t *testing.T) {
			i, _, o := start(t)
			switch field {
			case "kem":
				o.KEMPublicKey[0] ^= 1
			case "signature":
				o.Signature[0] ^= 1
			case "length":
				o.KEMPublicKey = o.KEMPublicKey[:32]
			}
			if _, _, e := i.Respond(o); !errors.Is(e, ErrRejected) {
				t.Fatal("tampered offer accepted")
			}
			if _, _, e := i.Respond(o); !errors.Is(e, ErrConsumed) {
				t.Fatal("failed state reused")
			}
		})
	}
}
func TestResponseTamperingConsumesAttempt(t *testing.T) {
	for _, field := range []string{"ciphertext", "signature", "length"} {
		t.Run(field, func(t *testing.T) {
			i, s, o := start(t)
			_, r, e := i.Respond(o)
			if e != nil {
				t.Fatal(e)
			}
			switch field {
			case "ciphertext":
				r.Ciphertext[0] ^= 1
			case "signature":
				r.Signature[0] ^= 1
			case "length":
				r.Ciphertext = r.Ciphertext[:32]
			}
			if _, _, e := s.Confirm(r); !errors.Is(e, ErrRejected) {
				t.Fatal("tampered response accepted")
			}
			if _, _, e := s.Confirm(r); !errors.Is(e, ErrConsumed) {
				t.Fatal("failed state reused")
			}
		})
	}
}
func TestBadConfirmationDoesNotReleaseKeys(t *testing.T) {
	i, s, o := start(t)
	pi, r, e := i.Respond(o)
	if e != nil {
		t.Fatal(e)
	}
	ps, c, e := s.Confirm(r)
	if e != nil {
		t.Fatal(e)
	}
	c[0] ^= 1
	if k, _, e := pi.Finish(c); !errors.Is(e, ErrRejected) || k != (SessionKeys{}) {
		t.Fatal("keys released on bad responder confirmation")
	}
	if k, e := ps.Finish(c); !errors.Is(e, ErrRejected) || k != (SessionKeys{}) {
		t.Fatal("keys released on bad initiator confirmation")
	}
}
func TestOfferCannotMoveBetweenAttempts(t *testing.T) {
	a, b := identity(t), identity(t)
	i1, h1, e := NewInitiator("fixture-chain", a, b.PublicKey())
	if e != nil {
		t.Fatal(e)
	}
	_, o1, e := NewResponder("fixture-chain", b, a.PublicKey(), h1)
	if e != nil {
		t.Fatal(e)
	}
	i2, h2, e := NewInitiator("fixture-chain", a, b.PublicKey())
	if e != nil {
		t.Fatal(e)
	}
	_, o2, e := NewResponder("fixture-chain", b, a.PublicKey(), h2)
	if e != nil {
		t.Fatal(e)
	}
	if bytes.Equal(h1.Nonce[:], h2.Nonce[:]) || bytes.Equal(o1.KEMPublicKey, o2.KEMPublicKey) {
		t.Fatal("fresh attempt reused random material")
	}
	if _, _, e := i2.Respond(o1); e == nil {
		t.Fatal("offer replay across attempts accepted")
	}
	if _, _, e := i1.Respond(o1); e != nil {
		t.Fatal(e)
	}
}
func TestCallerMutationDoesNotChangeStoredTranscript(t *testing.T) {
	a, b := identity(t), identity(t)
	i, h, e := NewInitiator("fixture-chain", a, b.PublicKey())
	if e != nil {
		t.Fatal(e)
	}
	s, o, e := NewResponder("fixture-chain", b, a.PublicKey(), h)
	if e != nil {
		t.Fatal(e)
	}
	h.Initiator[0] ^= 1
	h.Responder[0] ^= 1
	pi, r, e := i.Respond(o)
	if e != nil {
		t.Fatal(e)
	}
	o.KEMPublicKey[0] ^= 1
	o.Signature[0] ^= 1
	ps, c, e := s.Confirm(r)
	if e != nil {
		t.Fatal(e)
	}
	_, client, e := pi.Finish(c)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = ps.Finish(client); e != nil {
		t.Fatal(e)
	}
}
func TestProductionAlwaysBlocked(t *testing.T) {
	if !errors.Is(RequireProductionTransport(), ErrProductionBlocked) {
		t.Fatal("production gate opened")
	}
}

func TestCloseDiscardsAbandonedAttempts(t *testing.T) {
	i, s, o := start(t)
	i.Close()
	i.Close()
	s.Close()
	s.Close()
	if _, _, e := i.Respond(o); !errors.Is(e, ErrConsumed) {
		t.Fatal("closed initiator reused")
	}
	if _, _, e := s.Confirm(Response{}); !errors.Is(e, ErrConsumed) {
		t.Fatal("closed responder reused")
	}
	i, s, o = start(t)
	pi, r, e := i.Respond(o)
	if e != nil {
		t.Fatal(e)
	}
	ps, c, e := s.Confirm(r)
	if e != nil {
		t.Fatal(e)
	}
	a, b := pi.state, ps.state
	pi.Close()
	pi.Close()
	ps.Close()
	ps.Close()
	if *a != (pendingState{}) || *b != (pendingState{}) {
		t.Fatal("pending keys not cleared")
	}
	if _, _, e := pi.Finish(c); !errors.Is(e, ErrConsumed) {
		t.Fatal("closed initiator released keys")
	}
	if _, e := ps.Finish(c); !errors.Is(e, ErrConsumed) {
		t.Fatal("closed responder released keys")
	}
}
