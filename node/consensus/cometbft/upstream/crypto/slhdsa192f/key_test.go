package slhdsa192f

import (
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
)

func TestSizesMatchCircl(t *testing.T) {
	priv, err := GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	sig, err := priv.Sign([]byte("vote"))
	if err != nil {
		t.Fatal(err)
	}
	if len(priv.Bytes()) != PrivKeySize || len(priv.PubKey().Bytes()) != PubKeySize || len(sig) != SignatureSize {
		t.Fatalf("sizes: priv %d pub %d sig %d", len(priv.Bytes()), len(priv.PubKey().Bytes()), len(sig))
	}
}

func TestRoundTripSignVerify(t *testing.T) {
	priv, err := GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	again, err := NewPrivKeyFromBytes(priv.Bytes())
	if err != nil || !again.Equals(priv) {
		t.Fatalf("private key round trip: %v", err)
	}
	pub, err := NewPubKeyFromBytes(priv.PubKey().Bytes())
	if err != nil || !pub.Equals(priv.PubKey()) {
		t.Fatalf("public key round trip: %v", err)
	}
	msg := []byte("canonical vote sign bytes")
	sig, err := priv.Sign(msg)
	if err != nil {
		t.Fatal(err)
	}
	if !pub.VerifySignature(msg, sig) {
		t.Fatal("valid signature rejected")
	}
	sig[0] ^= 1
	if pub.VerifySignature(msg, sig) {
		t.Fatal("altered signature accepted")
	}
	if pub.VerifySignature(msg, sig[:len(sig)-1]) {
		t.Fatal("short signature accepted")
	}
	if _, err := NewPubKeyFromBytes(make([]byte, mldsa65.PubKeySize)); err == nil {
		t.Fatal("ML-DSA-65 length accepted as SLH-DSA key")
	}
}

// TestVoteCost reports per-vote signing time and the commit size a validator
// set would carry, against ML-DSA-65. Run with -v to see the figures.
func TestVoteCost(t *testing.T) {
	msg := make([]byte, 160) // about the size of canonical vote sign bytes
	slh, _ := GenPrivKey()
	ml, _ := mldsa65.GenPrivKey()
	const rounds = 5
	timeIt := func(sign func([]byte) ([]byte, error)) time.Duration {
		start := time.Now()
		for i := 0; i < rounds; i++ {
			if _, err := sign(msg); err != nil {
				t.Fatal(err)
			}
		}
		return time.Since(start) / rounds
	}
	slhSign, mlSign := timeIt(slh.Sign), timeIt(ml.Sign)
	sig, _ := slh.Sign(msg)
	start := time.Now()
	for i := 0; i < rounds; i++ {
		slh.PubKey().VerifySignature(msg, sig)
	}
	slhVerify := time.Since(start) / rounds
	t.Logf("sign per vote: slh-dsa-shake-192f %v, ml-dsa-65 %v", slhSign, mlSign)
	t.Logf("verify per vote: slh-dsa-shake-192f %v (a node verifies every validator's vote, twice per round)", slhVerify)
	// Each commit sig carries the signature plus about 40 bytes of
	// address, timestamp and framing (types.MaxCommitSigBytes).
	for _, n := range []int{4, 21, 50, 100, 150} {
		t.Logf("commit for %3d validators: slh-dsa-shake-192f %6.2f MiB, ml-dsa-65 %5.2f MiB; verify %v",
			n, float64(n*(SignatureSize+40))/(1<<20), float64(n*(mldsa65.SignatureSize+40))/(1<<20),
			time.Duration(n)*slhVerify)
	}
}
