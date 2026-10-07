package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"encoding/binary"
	"errors"
	"io"
	"strings"
	"testing"
)

func testBackup(t *testing.T, plain []byte, code []byte) []byte {
	t.Helper()
	var out bytes.Buffer
	writer, header, err := NewBackupWriter(&out, "dytallix-staging-1", 17280, code, rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	if header.Height != 17280 || header.ChunkBytes != BackupChunkBytes {
		t.Fatalf("header %+v", header)
	}
	// Uneven writes cross chunk boundaries.
	for rest := plain; len(rest) > 0; {
		n := min(len(rest), 300_001)
		if _, err := writer.Write(rest[:n]); err != nil {
			t.Fatal(err)
		}
		rest = rest[n:]
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

func openBackup(copyBytes []byte, code []byte) ([]byte, error) {
	reader, _, err := NewBackupReader(bytes.NewReader(copyBytes), code)
	if err != nil {
		return nil, err
	}
	return io.ReadAll(reader)
}

// records splits a copy into its header line and its records.
func records(t *testing.T, copyBytes []byte) ([]byte, [][]byte) {
	t.Helper()
	newline := bytes.IndexByte(copyBytes, '\n')
	header, rest := copyBytes[:newline+1], copyBytes[newline+1:]
	var out [][]byte
	for len(rest) > 0 {
		size := binary.BigEndian.Uint32(rest[:4])
		out = append(out, rest[:4+size])
		rest = rest[4+size:]
	}
	return header, out
}

func TestBackupRoundTripsAcrossChunks(t *testing.T) {
	code := testCode(9)
	for _, size := range []int{0, 1, BackupChunkBytes, BackupChunkBytes*2 + 12345} {
		plain := make([]byte, size)
		_, _ = rand.Read(plain)
		copyBytes := testBackup(t, plain, code)
		got, err := openBackup(copyBytes, code)
		if err != nil || !bytes.Equal(got, plain) {
			t.Fatalf("%d bytes: did not round trip: %v", size, err)
		}
		_, recs := records(t, copyBytes)
		if want := size/BackupChunkBytes + 1; len(recs) != want {
			t.Fatalf("%d bytes: %d records, want %d", size, len(recs), want)
		}
	}
	// Two copies of the same data differ: each has its own salt and key.
	plain := []byte("same snapshot")
	if bytes.Equal(testBackup(t, plain, code), testBackup(t, plain, code)) {
		t.Fatal("two copies are identical")
	}
}

func TestBackupRefusesDamageAndTheWrongCode(t *testing.T) {
	code := testCode(9)
	plain := make([]byte, BackupChunkBytes*2+77)
	_, _ = rand.Read(plain)
	copyBytes := testBackup(t, plain, code)
	header, recs := records(t, copyBytes)
	join := func(parts ...[]byte) []byte { return bytes.Join(parts, nil) }
	flipped := append([]byte(nil), copyBytes...)
	flipped[len(header)+100] ^= 1
	headerEdit := bytes.Replace(copyBytes, []byte(`"height":17280`), []byte(`"height":17281`), 1)
	cases := map[string][]byte{
		"truncated before the final chunk": join(header, recs[0], recs[1]),
		"cut inside a chunk":               copyBytes[:len(copyBytes)-10],
		"reordered":                        join(header, recs[1], recs[0], recs[2]),
		"a chunk dropped":                  join(header, recs[0], recs[2]),
		"data after the final chunk":       join(copyBytes, []byte{0}),
		"a ciphertext byte changed":        flipped,
		"another height in the header":     headerEdit,
		"no header":                        join(recs[0], recs[1], recs[2]),
	}
	for name, damaged := range cases {
		if _, err := openBackup(damaged, code); !errors.Is(err, ErrBackup) {
			t.Errorf("%s: opened (%v)", name, err)
		}
	}
	if _, err := openBackup(copyBytes, testCode(8)); !errors.Is(err, ErrBackup) {
		t.Fatal("another code opened the copy")
	}
	if _, _, err := NewBackupWriter(io.Discard, "Dytallix Staging", 1, code, rand.Reader); !errors.Is(err, ErrBackup) {
		t.Fatal("a malformed chain ID was accepted")
	}
	if _, _, err := NewBackupWriter(io.Discard, "dytallix-staging-1", 0, code, rand.Reader); !errors.Is(err, ErrBackup) {
		t.Fatal("height zero was accepted")
	}
}

func TestBackupPaperLine(t *testing.T) {
	code := []byte("0123456789abcdef0123456789abcdef")
	line, err := EncodeBackupCode("dytallix-mainnet-1", code)
	if err != nil {
		t.Fatal(err)
	}
	fields := strings.Fields(line)
	if fields[0] != "dytallix-backup-dytallix-mainnet-1" || len(fields) != 18 {
		t.Fatalf("unexpected paper line %q", line)
	}
	chain, decoded, err := DecodeBackupCode(strings.ToUpper(line[len(fields[0]):]) + "")
	if err == nil {
		t.Fatal("a line without its chain decoded", chain)
	}
	chain, decoded, err = DecodeBackupCode(fields[0] + strings.ToUpper(line[len(fields[0]):]))
	if err != nil || chain != "dytallix-mainnet-1" || !bytes.Equal(decoded, code) {
		t.Fatal("the paper line did not round trip", err)
	}
	if _, _, err := DecodeBackupCode(strings.Replace(line, "mainnet-1", "mainnet-2", 1)); !errors.Is(err, ErrBackup) {
		t.Fatal("another chain's line passed the check group")
	}
	// A seal code line is not a backup code line.
	seal, _ := EncodeSealCode("validator-1", code)
	if _, _, err := DecodeBackupCode(seal); !errors.Is(err, ErrBackup) {
		t.Fatal("a seal code decoded as a backup code")
	}
}
