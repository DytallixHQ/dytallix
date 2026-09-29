// Package clientchannel_test reproduces the Rust client channel's fixed-seed
// vector (node/crates/client-channel, E04 gap 19) with independent providers:
// CIRCL's ML-KEM-768 and ML-DSA-65 and Go's HKDF and AES-256-GCM. The two
// implementations share only the specification in
// docs/architecture/client-channel-v1.md.
package clientchannel_test

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/hkdf"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"testing"

	"github.com/cloudflare/circl/kem/mlkem/mlkem768"
	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

const (
	suite   = "dytallix-client-channel-v1/mlkem768/mldsa65/hkdfsha256/aes256gcm"
	network = "dytallix-channel-test"
	// The Rust test fixed_seeds_give_the_recorded_transcript records the
	// same digest.
	recorded = "74a0f0cae5e6ae2effe560bb09a8191384019f4e99d27d2b9c42c37616c759f8"
)

func fill(b byte) []byte { return bytes.Repeat([]byte{b}, 32) }

func concat(parts ...[]byte) []byte { return bytes.Join(parts, nil) }

func encode(parts ...[]byte) []byte {
	var out []byte
	for _, p := range parts {
		out = binary.BigEndian.AppendUint32(out, uint32(len(p)))
		out = append(out, p...)
	}
	return out
}

func frame(kind byte, payload []byte) []byte {
	out := append([]byte("DYCH"), 1, kind)
	out = binary.BigEndian.AppendUint16(out, uint16(len(payload)))
	return append(out, payload...)
}

func seal(t *testing.T, key, message []byte) []byte {
	block, err := aes.NewCipher(key)
	if err != nil {
		t.Fatal(err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatal(err)
	}
	var out []byte
	for seq := uint64(0); len(message) > 0; seq++ {
		n := min(len(message), 16384)
		header := append([]byte("DYCR"), 1, 0)
		if n == len(message) {
			header[5] = 1
		}
		header = binary.BigEndian.AppendUint64(header, seq)
		header = binary.BigEndian.AppendUint16(header, uint16(n))
		nonce := binary.BigEndian.AppendUint64(make([]byte, 4), seq)
		out = append(out, header...)
		out = gcm.Seal(out, nonce, message[:n], header)
		message = message[n:]
	}
	return out
}

func field(width int, value []byte) []byte {
	var n [4]byte
	binary.BigEndian.PutUint32(n[:], uint32(len(value)))
	return append(n[4-width:], value...)
}

func TestFixedSeedsGiveTheRecordedTranscript(t *testing.T) {
	var seed [mldsa65.SeedSize]byte
	copy(seed[:], fill(0x11))
	endpointKey, signingKey := mldsa65.NewKeyFromSeed(&seed)
	endpoint := endpointKey.Bytes()

	nonce := fill(0x21)
	encapsulation, decapsulation := mlkem768.NewKeyFromSeed(concat(fill(0x22), fill(0x23)))
	ek := make([]byte, mlkem768.PublicKeySize)
	encapsulation.Pack(ek)
	hello := frame(1, concat([]byte{byte(len(network))}, []byte(network), nonce, endpoint, ek))

	ciphertext := make([]byte, mlkem768.CiphertextSize)
	secret := make([]byte, mlkem768.SharedKeySize)
	encapsulation.EncapsulateTo(ciphertext, secret, fill(0x31))
	message := encode([]byte(suite), []byte(network), nonce, endpoint, ek, ciphertext)
	signature := make([]byte, mldsa65.SignatureSize)
	if err := mldsa65.SignTo(signingKey, message, []byte(suite+"/endpoint-offer"), false, signature); err != nil {
		t.Fatal(err)
	}
	transcript := sha256.Sum256(encode(message, signature))
	material, err := hkdf.Key(sha256.New, secret, transcript[:], suite+"/traffic-and-confirmation", 128)
	if err != nil {
		t.Fatal(err)
	}
	decapsulated := make([]byte, mlkem768.SharedKeySize)
	decapsulation.DecapsulateTo(decapsulated, ciphertext)
	if !bytes.Equal(decapsulated, secret) || !mldsa65.Verify(endpointKey, message, []byte(suite+"/endpoint-offer"), signature) {
		t.Fatal("the vector does not verify")
	}
	offer := frame(2, concat(ciphertext, signature, material[64:96]))
	finish := frame(3, material[96:128])

	body := []byte(`{"jsonrpc":"2.0","id":1,"method":"status","params":{}}`)
	request := seal(t, material[0:32], concat([]byte{1, 2}, field(2, []byte("/")), field(2, nil), field(4, body)))
	response := seal(t, material[32:64], concat([]byte{2, 0, 200}, field(2, []byte("application/json")), field(2, nil), field(4, []byte("{}"))))

	digest := sha256.New()
	for _, part := range [][]byte{hello, offer, finish, request, response} {
		digest.Write(binary.BigEndian.AppendUint32(nil, uint32(len(part))))
		digest.Write(part)
	}
	if got := hex.EncodeToString(digest.Sum(nil)); got != recorded {
		t.Fatalf("digest %s, recorded %s", got, recorded)
	}
}
