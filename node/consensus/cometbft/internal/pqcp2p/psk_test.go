package pqcp2p

import (
	"bytes"
	"crypto/mlkem"
	"errors"
	"net"
	"testing"
	"time"
)

// pskPair upgrades a net.Pipe with each side's own view of the pair's PSK.
func pskPair(t *testing.T, a, b *Identity, aKey, bKey PSK) (*Conn, *Conn, error, error) {
	t.Helper()
	left, right := net.Pipe()
	result := make(chan upgraded, 1)
	go func() {
		c, e := UpgradePSK(right, "fixture-network", b, [][]byte{a.PublicKey()}, nil,
			func([]byte) PSK { return bKey }, 3*time.Second)
		result <- upgraded{c, e}
	}()
	c, e := UpgradePSK(left, "fixture-network", a, [][]byte{b.PublicKey()}, b.PublicKey(),
		func([]byte) PSK { return aKey }, 3*time.Second)
	r := <-result
	return c, r.conn, e, r.err
}

func TestMatchingPSKCarriesTraffic(t *testing.T) {
	a, b := identity(t), identity(t)
	psk := PSK{1, 2, 3}
	left, right, e1, e2 := pskPair(t, a, b, psk, psk)
	if e1 != nil || e2 != nil {
		t.Fatalf("matching PSK rejected: %v %v", e1, e2)
	}
	defer left.Close()
	defer right.Close()
	go func() { _, _ = left.Write([]byte("vote")) }()
	got := make([]byte, 4)
	if _, err := right.Read(got); err != nil || string(got) != "vote" {
		t.Fatalf("read %q, %v", got, err)
	}
}

// An attacker who can forge ML-DSA-65 is modelled by holding a pinned
// identity's real private key. It completes the signed handshake and the
// ML-KEM exchange, and still fails key confirmation without the pair's PSK.
func TestForgedIdentityWithoutPSKIsRefused(t *testing.T) {
	a, b := identity(t), identity(t)
	for _, guess := range []PSK{{}, {9}} {
		left, right, e1, e2 := pskPair(t, a, b, guess, PSK{1, 2, 3})
		if left != nil || right != nil || !errors.Is(e1, ErrRejected) && !errors.Is(e2, ErrRejected) {
			t.Fatalf("PSK mismatch accepted with guess %v: %v %v", guess[:1], e1, e2)
		}
	}
}

// A passive attacker who recovers the ML-KEM shared secret from the
// transcript still derives different traffic keys without the PSK.
func TestKEMSecretAloneDoesNotGiveTrafficKeys(t *testing.T) {
	a, b := identity(t), identity(t)
	psk := PSK{7}
	i, h, err := NewInitiatorPSK("fixture-network", a, b.PublicKey(), psk)
	if err != nil {
		t.Fatal(err)
	}
	r, offer, err := NewResponderPSK("fixture-network", b, a.PublicKey(), h, psk)
	if err != nil {
		t.Fatal(err)
	}
	ek, err := mlkem.NewEncapsulationKey768(offer.KEMPublicKey)
	if err != nil {
		t.Fatal(err)
	}
	// The "broken" KEM: the attacker knows a shared secret and its ciphertext.
	secret, ct := ek.Encapsulate()
	resp := Response{Ciphertext: ct}
	resp.Signature, err = sign(a, responseMessage(h, offer, resp), "initiator-response")
	if err != nil {
		t.Fatal(err)
	}
	pending, server, err := r.Confirm(resp)
	if err != nil {
		t.Fatal(err)
	}
	defer pending.Close()
	attacker, err := derive(secret, PSK{}, h, offer, resp, true)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Equal(attacker.server[:], server[:]) {
		t.Fatal("KEM secret without the PSK reproduced key confirmation")
	}
	withPSK, _ := derive(secret, psk, h, offer, resp, true)
	if !bytes.Equal(withPSK.server[:], server[:]) {
		t.Fatal("KEM secret with the PSK should reproduce the responder's keys")
	}
	i.Close()
}

func TestVersion2HandshakeIsRefused(t *testing.T) {
	h := []byte{'D', 'Y', 'P', 'H', 2, helloType, 15, 98}
	if _, err := readHandshake(bytes.NewReader(h), helloType); !errors.Is(err, ErrRejected) {
		t.Fatalf("version 2 header accepted: %v", err)
	}
}
