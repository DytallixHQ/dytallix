package rootauthorization

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/cloudflare/circl/sign/slhdsa"
)

func testSecret(seed byte) []byte { return bytes.Repeat([]byte{seed}, KitSecretBytes) }

func TestKitKeysAreDeterministicDistinctAndValid(t *testing.T) {
	seen := map[string]string{}
	for kit := 1; kit <= KitCount; kit++ {
		for _, purpose := range KitPurposes {
			public, private, err := DeriveKitKey(testSecret(7), kit, purpose)
			if err != nil {
				t.Fatal(err)
			}
			again, _, err := DeriveKitKey(testSecret(7), kit, purpose)
			if err != nil || !bytes.Equal(public, again) {
				t.Fatal("derivation is not deterministic", err)
			}
			// Signing is slow; one kit's pairs sign, and the rest compare.
			if kit == 1 {
				if err := CheckKeyPair(private, public); err != nil {
					t.Fatalf("kit %d %s: %v", kit, purpose, err)
				}
			} else if len(private) != PrivateKeySize || !bytes.Equal(private[64:], public) {
				t.Fatalf("kit %d %s: the private key does not embed its public key", kit, purpose)
			}
			id := KeyID(public)
			if other, ok := seen[id]; ok {
				t.Fatalf("kit %d %s repeats %s", kit, purpose, other)
			}
			seen[id] = purpose
		}
	}
	other, _, _ := DeriveKitKey(testSecret(8), 1, "genesis")
	first, _, _ := DeriveKitKey(testSecret(7), 1, "genesis")
	if bytes.Equal(other, first) {
		t.Fatal("different kit secrets gave the same key")
	}
	for _, bad := range []struct {
		secret  []byte
		kit     int
		purpose string
	}{{testSecret(7)[:31], 1, "genesis"}, {testSecret(7), 0, "genesis"}, {testSecret(7), 6, "genesis"}, {testSecret(7), 1, "handover"}} {
		if _, _, err := DeriveKitKey(bad.secret, bad.kit, bad.purpose); !errors.Is(err, ErrKit) {
			t.Fatalf("accepted %d %q", bad.kit, bad.purpose)
		}
	}
}

// The derivation is FIPS 205 key generation from SHAKE256-derived seeds,
// pinned so a library update cannot silently change every kit.
func TestKitDerivationIsPinned(t *testing.T) {
	public, _, err := DeriveKitKey(testSecret(7), 3, "freeze")
	if err != nil {
		t.Fatal(err)
	}
	seeds := make([]byte, 96)
	shake(seeds, []byte(kitDomain), []byte{3}, []byte("freeze"), []byte{0}, testSecret(7))
	pub, _, err := slhdsa.GenerateKey(bytes.NewReader(seeds), slhdsa.SHAKE_256s)
	if err != nil {
		t.Fatal(err)
	}
	expected, _ := pub.MarshalBinary()
	if !bytes.Equal(public, expected) {
		t.Fatal("the kit key is not FIPS 205 key generation from the derived seeds")
	}
	// PK.seed is the third 32-byte seed and the first half of the public key.
	if !bytes.Equal(public[:32], seeds[64:96]) {
		t.Fatal("PK.seed is not where FIPS 205 puts it")
	}
	if got := KeyID(public); got != pinnedKitKeyID {
		t.Fatalf("kit derivation changed: %s", got)
	}
}

const pinnedKitKeyID = "cb13c8658cb04270ff6de7405060538eedf39687b55e573a809d8a13dbdb382a"

func TestKitPaperLine(t *testing.T) {
	line, err := EncodeKitSecret(4, testSecret(9))
	if err != nil {
		t.Fatal(err)
	}
	fields := strings.Fields(line)
	if fields[0] != "dytallix-kit-4" || len(fields) != 18 {
		t.Fatalf("unexpected paper line %q", line)
	}
	kit, secret, err := DecodeKitSecret("  " + strings.ToUpper(strings.Join(fields, "   ")) + "\n")
	if err != nil || kit != 4 || !bytes.Equal(secret, testSecret(9)) {
		t.Fatal("paper line does not read back", err)
	}
	// A mistyped digit, a swapped kit number or a missing group fails.
	typo := []byte(line)
	typo[len("dytallix-kit-4 ")] ^= 1
	for _, bad := range []string{string(typo), strings.Replace(line, "kit-4", "kit-2", 1), strings.Join(fields[:17], " "), "dytallix-kit-9 " + strings.Join(fields[1:], " "), "kit-4 " + strings.Join(fields[1:], " ")} {
		if _, _, err := DecodeKitSecret(bad); !errors.Is(err, ErrKit) {
			t.Fatalf("accepted %q", bad)
		}
	}
	if _, err := EncodeKitSecret(6, testSecret(9)); err == nil {
		t.Fatal("encoded kit 6")
	}
}

func epoch(n uint64) *uint64 { return &n }

func TestPossessionProof(t *testing.T) {
	public, private, err := DeriveKitKey(testSecret(5), 2, "upgrade")
	if err != nil {
		t.Fatal(err)
	}
	challenge := PossessionChallenge{Schema: PossessionSchema, ChainID: "dytallix-mainnet-1", ControllerID: "founder",
		Purpose: "upgrade", AuthorityEpoch: epoch(1), KeyID: KeyID(public), Session: "ceremony-1"}
	proof, err := ProvePossession(challenge, private)
	if err != nil {
		t.Fatal(err)
	}
	if err := VerifyPossession(proof); err != nil {
		t.Fatal(err)
	}
	tampered := proof
	tampered.Challenge.Session = "ceremony-2"
	if VerifyPossession(tampered) == nil {
		t.Fatal("a changed challenge verified")
	}
	tampered = proof
	tampered.Challenge.AuthorityEpoch = epoch(2)
	if VerifyPossession(tampered) == nil {
		t.Fatal("a changed epoch verified")
	}
	otherPublic, otherPrivate, _ := DeriveKitKey(testSecret(5), 2, "freeze")
	if _, err := ProvePossession(challenge, otherPrivate); err == nil {
		t.Fatal("proved a key ID with another key")
	}
	swapped := proof
	swapped.PublicKeyHex = hex.EncodeToString(otherPublic)
	if VerifyPossession(swapped) == nil {
		t.Fatal("a proof verified under another public key")
	}
	// Genesis signers have no epoch; the other purposes need one.
	for _, bad := range []PossessionChallenge{
		{Schema: PossessionSchema, ChainID: "dytallix-mainnet-1", ControllerID: "founder", Purpose: "genesis", AuthorityEpoch: epoch(1), KeyID: challenge.KeyID, Session: "s"},
		{Schema: PossessionSchema, ChainID: "dytallix-mainnet-1", ControllerID: "founder", Purpose: "freeze", KeyID: challenge.KeyID, Session: "s"},
		{Schema: PossessionSchema, ChainID: "dytallix-mainnet-1", ControllerID: "founder", Purpose: "upgrade", AuthorityEpoch: epoch(0), KeyID: challenge.KeyID, Session: "s"},
		{Schema: PossessionSchema, ChainID: "has space", ControllerID: "founder", Purpose: "upgrade", AuthorityEpoch: epoch(1), KeyID: challenge.KeyID, Session: "s"},
	} {
		if _, err := ProvePossession(bad, private); err == nil {
			t.Fatalf("accepted challenge %+v", bad)
		}
	}
}

// A proof's signature is under its own FIPS 205 context, so it is no root
// action, root genesis or key validation signature.
func TestPossessionSignaturesAreDomainSeparated(t *testing.T) {
	public, private, _ := DeriveKitKey(testSecret(6), 1, "freeze")
	challenge := PossessionChallenge{Schema: PossessionSchema, ChainID: "dytallix-mainnet-1", ControllerID: "founder",
		Purpose: "freeze", AuthorityEpoch: epoch(1), KeyID: KeyID(public), Session: "ceremony-1"}
	proof, err := ProvePossession(challenge, private)
	if err != nil {
		t.Fatal(err)
	}
	signature, _ := hex.DecodeString(proof.SignatureHex)
	key := slhdsa.PublicKey{ID: slhdsa.SHAKE_256s}
	if err := key.UnmarshalBinary(public); err != nil {
		t.Fatal(err)
	}
	message := slhdsa.NewMessage([]byte(mustJSON(t, challenge)))
	for _, context := range []string{"", "DYTALLIX/ROOT/v1/genesis", "DYTALLIX/ROOT/v1/emergency", "DYTALLIX/ROOT/v1/upgrade", "DYTALLIX/ROOT/KEY-VALIDATION/v1"} {
		if slhdsa.Verify(&key, message, signature, []byte(context)) {
			t.Fatalf("the proof verifies under context %q", context)
		}
	}
}

func mustJSON(t *testing.T, value any) string {
	t.Helper()
	raw, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}
