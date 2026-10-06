package rootauthorization

import (
	"bytes"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"

	"github.com/cloudflare/circl/sign/slhdsa"
	"github.com/cloudflare/circl/xof"
)

// Key kits (solo launch profile, P01, 3 October 2026): the founder holds
// every root key in five kits, and kit N holds key N of each role. Each kit's
// four keys derive from one 32-byte kit secret, so the kit's paper copy is
// the secret alone (P01, 5 October 2026: an encrypted drive plus a paper copy
// per kit).
//
// Derivation: SHAKE256(KitDomain || kit || purpose || 0x00 || secret) gives
// the 96 bytes SK.seed || SK.prf || PK.seed, and the key pair is FIPS 205
// slh_keygen_internal of those seeds (Algorithm 21 reads them in that order).
// A kit's keys are independent of other kits' and of the other purposes'.
const (
	KitCount       = 5
	KitSecretBytes = 32
	kitDomain      = "DYTALLIX/KEY-KIT/v1\x00"
	kitCheckDomain = "DYTALLIX/KEY-KIT-CHECK/v1\x00"
	kitPaperPrefix = "dytallix-kit-"
	kitCheckBytes  = 2
)

// KitPurposes are the four root roles a kit holds a key for.
var KitPurposes = []string{"genesis", "upgrade", "freeze", "resume"}

var ErrKit = errors.New("invalid key kit")

func kitPurpose(purpose string) bool {
	for _, p := range KitPurposes {
		if p == purpose {
			return true
		}
	}
	return false
}

func shake(out []byte, parts ...[]byte) {
	h := xof.SHAKE256.New()
	for _, part := range parts {
		_, _ = h.Write(part)
	}
	_, _ = h.Read(out)
}

// DeriveKitKey returns the raw public and private key of one kit's purpose.
func DeriveKitKey(secret []byte, kit int, purpose string) (public, private []byte, err error) {
	if len(secret) != KitSecretBytes || kit < 1 || kit > KitCount || !kitPurpose(purpose) {
		return nil, nil, ErrKit
	}
	seeds := make([]byte, 3*32)
	defer clear(seeds)
	shake(seeds, []byte(kitDomain), []byte{byte(kit)}, []byte(purpose), []byte{0}, secret)
	pub, priv, err := slhdsa.GenerateKey(bytes.NewReader(seeds), slhdsa.SHAKE_256s)
	if err != nil {
		return nil, nil, err
	}
	if public, err = pub.MarshalBinary(); err != nil {
		return nil, nil, err
	}
	if private, err = priv.MarshalBinary(); err != nil {
		return nil, nil, err
	}
	return public, private, nil
}

func kitCheck(kit int, secret []byte) []byte {
	sum := make([]byte, kitCheckBytes)
	shake(sum, []byte(kitCheckDomain), []byte{byte(kit)}, secret)
	return sum
}

// EncodeKitSecret is the paper line for a kit: its number, the secret in
// sixteen groups of four hex digits, and a check group bound to the number.
func EncodeKitSecret(kit int, secret []byte) (string, error) {
	if len(secret) != KitSecretBytes || kit < 1 || kit > KitCount {
		return "", ErrKit
	}
	digits := hex.EncodeToString(append(append([]byte{}, secret...), kitCheck(kit, secret)...))
	groups := make([]string, 0, len(digits)/4)
	for i := 0; i < len(digits); i += 4 {
		groups = append(groups, digits[i:i+4])
	}
	return fmt.Sprintf("%s%d %s", kitPaperPrefix, kit, strings.Join(groups, " ")), nil
}

// DecodeKitSecret reads a paper line back, ignoring spacing and case. A
// mistyped digit or the wrong kit number fails the check group (one in 65,536
// errors would pass it; the public key IDs then catch the rest).
func DecodeKitSecret(line string) (int, []byte, error) {
	fields := strings.Fields(strings.ToLower(line))
	if len(fields) < 2 || !strings.HasPrefix(fields[0], kitPaperPrefix) {
		return 0, nil, ErrKit
	}
	var kit int
	if _, err := fmt.Sscanf(strings.TrimPrefix(fields[0], kitPaperPrefix), "%d", &kit); err != nil || kit < 1 || kit > KitCount ||
		fields[0] != fmt.Sprintf("%s%d", kitPaperPrefix, kit) {
		return 0, nil, ErrKit
	}
	raw, err := hex.DecodeString(strings.Join(fields[1:], ""))
	if err != nil || len(raw) != KitSecretBytes+kitCheckBytes {
		clear(raw)
		return 0, nil, ErrKit
	}
	secret := raw[:KitSecretBytes]
	if !bytes.Equal(kitCheck(kit, secret), raw[KitSecretBytes:]) {
		clear(raw)
		return 0, nil, fmt.Errorf("%w: the check group does not match; recheck every digit and the kit number", ErrKit)
	}
	return kit, secret, nil
}
