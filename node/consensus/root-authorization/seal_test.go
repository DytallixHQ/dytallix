package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"errors"
	"strings"
	"testing"
)

func testHostFiles() []SecretFile {
	return []SecretFile{
		{Path: "data/priv_validator_state.json", Data: []byte(`{"height":"0","round":0,"step":0}`)},
		{Path: "config/pqc_peer_seed.bin", Data: bytes.Repeat([]byte{1}, 32)},
		{Path: "config/priv_validator_key.json", Data: []byte(`{"synthetic":"validator key"}`)},
	}
}

func testCode(seed byte) []byte { return bytes.Repeat([]byte{seed}, SealCodeBytes) }

func TestSealedHostKeysOpenOnlyForTheirHostAndCode(t *testing.T) {
	record, err := SealHostKeys("validator-1", testCode(3), testHostFiles(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	if record.Files[0].Path != "config/pqc_peer_seed.bin" || len(record.Files) != 3 {
		t.Fatalf("files are not sorted by path: %+v", record.Files)
	}
	files, err := OpenHostKeys(record, "validator-1", testCode(3))
	if err != nil {
		t.Fatal(err)
	}
	for i, want := range []string{"config/pqc_peer_seed.bin", "config/priv_validator_key.json", "data/priv_validator_state.json"} {
		if files[i].Path != want {
			t.Fatalf("file %d is %s", i, files[i].Path)
		}
	}
	if !bytes.Equal(files[2].Data, testHostFiles()[0].Data) {
		t.Fatal("the signing state did not round trip")
	}
	if _, err := OpenHostKeys(record, "validator-1", testCode(4)); !errors.Is(err, ErrSeal) {
		t.Fatal("another code opened the record", err)
	}
	if _, err := OpenHostKeys(record, "sentry-1", testCode(3)); !errors.Is(err, ErrSeal) {
		t.Fatal("the record opened for another host", err)
	}
	// Relabeling the record changes the key and the additional data.
	other := record
	other.Label = "sentry-1"
	if _, err := OpenHostKeys(other, "sentry-1", testCode(3)); !errors.Is(err, ErrSeal) {
		t.Fatal("a relabeled record opened", err)
	}
	// The header is authenticated: a changed digest or path fails.
	tampered := record
	tampered.Files = append([]SealedEntry(nil), record.Files...)
	tampered.Files[1].SHA256 = strings.Repeat("ab", 32)
	if _, err := OpenHostKeys(tampered, "validator-1", testCode(3)); !errors.Is(err, ErrSeal) {
		t.Fatal("a changed digest opened", err)
	}
	flipped := record
	flipped.CiphertextHex = "ff" + record.CiphertextHex[2:]
	if record.CiphertextHex[:2] == "ff" {
		flipped.CiphertextHex = "00" + record.CiphertextHex[2:]
	}
	if _, err := OpenHostKeys(flipped, "validator-1", testCode(3)); !errors.Is(err, ErrSeal) {
		t.Fatal("a changed ciphertext opened", err)
	}
}

func TestSealRefusesBadFiles(t *testing.T) {
	cases := map[string][]SecretFile{
		"absolute":  {{Path: "/etc/shadow", Data: []byte("x")}},
		"parent":    {{Path: "config/a..b", Data: []byte("x")}},
		"outside":   {{Path: "abci/state", Data: []byte("x")}},
		"nested":    {{Path: "config/sub/file", Data: []byte("x")}},
		"empty":     {{Path: "config/empty", Data: nil}},
		"duplicate": {{Path: "config/a", Data: []byte("x")}, {Path: "config/a", Data: []byte("y")}},
		"large":     {{Path: "config/large", Data: make([]byte, MaxSealedFileBytes+1)}},
		"none":      nil,
	}
	for name, files := range cases {
		if _, err := SealHostKeys("validator-1", testCode(3), files, rand.Reader); !errors.Is(err, ErrSeal) {
			t.Errorf("%s: sealed: %v", name, err)
		}
	}
	if _, err := SealHostKeys("bad label", testCode(3), testHostFiles(), rand.Reader); !errors.Is(err, ErrSeal) {
		t.Error("a bad label sealed")
	}
	if _, err := SealHostKeys("validator-1", testCode(3)[:16], testHostFiles(), rand.Reader); !errors.Is(err, ErrSeal) {
		t.Error("a 128-bit code sealed")
	}
}

func TestSealPaperLineRoundTripsAndCatchesTypos(t *testing.T) {
	code := []byte("0123456789abcdef0123456789abcdef")
	line, err := EncodeSealCode("endpoint-1", code)
	if err != nil {
		t.Fatal(err)
	}
	fields := strings.Fields(line)
	if fields[0] != "dytallix-seal-endpoint-1" || len(fields) != 1+16+1 {
		t.Fatalf("unexpected paper line %q", line)
	}
	label, decoded, err := DecodeSealCode("  " + strings.ToUpper(strings.Join(fields[1:], "")) + "\n")
	if err == nil {
		t.Fatal("a line without its label decoded", label)
	}
	label, decoded, err = DecodeSealCode(fields[0] + " " + strings.ToUpper(strings.Join(fields[1:], "")) + "\n")
	if err != nil || label != "endpoint-1" || !bytes.Equal(decoded, code) {
		t.Fatal("the paper line did not round trip", err)
	}
	typo := []byte(line)
	typo[len(typo)-6] ^= 1 // a digit of the last code group
	if _, _, err := DecodeSealCode(string(typo)); !errors.Is(err, ErrSeal) {
		t.Fatal("a typo passed the check group")
	}
	if _, _, err := DecodeSealCode(strings.Replace(line, "endpoint-1", "endpoint-2", 1)); !errors.Is(err, ErrSeal) {
		t.Fatal("another host's label passed the check group")
	}
	if _, _, err := DecodeSealCode(strings.Join(fields[:len(fields)-1], " ")); !errors.Is(err, ErrSeal) {
		t.Fatal("a line without its check group decoded")
	}
}
