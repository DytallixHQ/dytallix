package pqcp2p

import (
	"bytes"
	"errors"
	"testing"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

func TestImportSeedIdentityRequiresExactSeedAndFullPin(t *testing.T) {
	seed := make([]byte, mldsa65.SeedSize)
	for i := range seed {
		seed[i] = byte(i + 1)
	}
	var fixed [mldsa65.SeedSize]byte
	copy(fixed[:], seed)
	public, _ := mldsa65.NewKeyFromSeed(&fixed)
	pin := public.Bytes()
	imported, err := ImportSeedIdentity(seed, pin)
	if err != nil || imported == nil || !bytes.Equal(imported.PublicKey(), pin) {
		t.Fatalf("seed-derived identity differs from full pin: %v", err)
	}
	if !bytes.Equal(seed, fixed[:]) {
		t.Fatal("import changed caller-owned seed")
	}
	remote := identity(t)
	client, server := upgradePair(t, imported, remote, false, false)
	if !bytes.Equal(client.PeerPublicKey(), remote.PublicKey()) || !bytes.Equal(server.PeerPublicKey(), pin) {
		t.Fatal("seed-derived identity did not authenticate its pinned peer")
	}
	for _, input := range [][]byte{seed[:len(seed)-1], make([]byte, mldsa65.PrivateKeySize)} {
		if _, err := ImportSeedIdentity(input, pin); !errors.Is(err, ErrRejected) {
			t.Fatalf("invalid seed length accepted: %v", err)
		}
	}
	if _, err := ImportSeedIdentity(seed, pin[:len(pin)-1]); !errors.Is(err, ErrRejected) {
		t.Fatalf("short public pin accepted: %v", err)
	}
	wrong := bytes.Clone(pin)
	wrong[0] ^= 1
	if _, err := ImportSeedIdentity(seed, wrong); !errors.Is(err, ErrRejected) {
		t.Fatalf("wrong full pin accepted: %v", err)
	}
	otherSeed := bytes.Clone(seed)
	otherSeed[0] ^= 1
	if _, err := ImportSeedIdentity(otherSeed, pin); !errors.Is(err, ErrRejected) {
		t.Fatalf("wrong seed accepted for pinned peer: %v", err)
	}
}
