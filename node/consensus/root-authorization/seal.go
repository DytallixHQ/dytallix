package rootauthorization

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"sort"
	"strings"
)

// Sealed host keys (host setup v1; P01, 6 October 2026). Each host's node
// keys travel in its public bundle, encrypted under the host's seal code: 32
// random bytes the founder writes on paper, one code per host, two copies
// kept with two kits' papers. The paper line names its host:
//
//	dytallix-seal-LABEL xxxx xxxx ... (sixteen groups) cccc
//
// The code is the only secret and is uniformly random, so no password
// stretching is needed: SHAKE256(sealDomain || len(label) || label || code)
// gives the AES-256-GCM key. The additional data is the record's header (the
// schema, the label and each file's path, size and SHA-256), so a sealed
// record opens only for its host and only with the files it names.
const (
	SealSchema         = "dytallix.sealed-host-keys.v1"
	SealCodeBytes      = 32
	MaxSealedFiles     = 8
	MaxSealedFileBytes = 64 << 10
	sealDomain         = "DYTALLIX/HOST-SEAL/v1\x00"
	sealCheckDomain    = "DYTALLIX/HOST-SEAL-CHECK/v1\x00"
	sealPaperPrefix    = "dytallix-seal-"
	sealNonceBytes     = 12
)

var ErrSeal = errors.New("invalid sealed host keys")

// sealLabel is the pin plan's host label (dytallix-host-config).
var sealLabel = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`)

// sealPath is a secret file by where it goes: the node home's config or
// data directory, the sentry's backup secrets (the backup code and the
// upload key, disaster recovery v1), or every host's history secrets (the
// history code and its upload key, monitoring v1), which the installer
// places outside the node home.
var sealPath = regexp.MustCompile(`^(config|data|backup|history)/[a-z0-9][a-z0-9_.-]{0,127}$`)

// SecretFile is one secret file, by its path relative to the node home.
type SecretFile struct {
	Path string
	Data []byte
}

// SealedEntry names one sealed file. Its digest is public, as in the host's
// key summary (dytallix.host-keys.v1).
type SealedEntry struct {
	Path   string `json:"path"`
	Bytes  int    `json:"bytes"`
	SHA256 string `json:"sha256"`
}

// SealedHostKeys is the public record a host bundle carries.
type SealedHostKeys struct {
	Schema        string        `json:"schema"`
	Label         string        `json:"label"`
	Files         []SealedEntry `json:"files"`
	NonceHex      string        `json:"nonce_hex"`
	CiphertextHex string        `json:"ciphertext_hex"`
}

type sealHeader struct {
	Schema string        `json:"schema"`
	Label  string        `json:"label"`
	Files  []SealedEntry `json:"files"`
}

func (s SealedHostKeys) header() ([]byte, error) {
	return json.Marshal(sealHeader{Schema: s.Schema, Label: s.Label, Files: s.Files})
}

// Validate checks the record's shape, not its contents.
func (s SealedHostKeys) Validate() error {
	if s.Schema != SealSchema || !sealLabel.MatchString(s.Label) || len(s.Files) == 0 || len(s.Files) > MaxSealedFiles {
		return ErrSeal
	}
	total := 0
	for i, file := range s.Files {
		if !sealPath.MatchString(file.Path) || strings.Contains(file.Path, "..") || (i > 0 && s.Files[i-1].Path >= file.Path) ||
			file.Bytes < 1 || file.Bytes > MaxSealedFileBytes || !hexDigest(file.SHA256, sha256.Size) {
			return fmt.Errorf("%w: file %d", ErrSeal, i)
		}
		total += file.Bytes
	}
	nonce, err := hex.DecodeString(s.NonceHex)
	if err != nil || len(nonce) != sealNonceBytes || hex.EncodeToString(nonce) != s.NonceHex {
		return fmt.Errorf("%w: nonce", ErrSeal)
	}
	if len(s.CiphertextHex) != 2*(total+16) || strings.ToLower(s.CiphertextHex) != s.CiphertextHex {
		return fmt.Errorf("%w: ciphertext size", ErrSeal)
	}
	return nil
}

func hexDigest(value string, size int) bool {
	raw, err := hex.DecodeString(value)
	return err == nil && len(raw) == size && hex.EncodeToString(raw) == value
}

func sealAEAD(label string, code []byte) (cipher.AEAD, error) {
	if len(code) != SealCodeBytes || !sealLabel.MatchString(label) {
		return nil, ErrSeal
	}
	key := make([]byte, 32)
	defer clear(key)
	shake(key, []byte(sealDomain), []byte{byte(len(label))}, []byte(label), code)
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	return cipher.NewGCM(block)
}

// SealHostKeys encrypts a host's secret files under its seal code. The
// nonce comes from random; each code seals once.
func SealHostKeys(label string, code []byte, files []SecretFile, random io.Reader) (SealedHostKeys, error) {
	sorted := append([]SecretFile(nil), files...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].Path < sorted[j].Path })
	record := SealedHostKeys{Schema: SealSchema, Label: label}
	var plain bytes.Buffer
	defer func() { clear(plain.Bytes()) }()
	for _, file := range sorted {
		sum := sha256.Sum256(file.Data)
		record.Files = append(record.Files, SealedEntry{Path: file.Path, Bytes: len(file.Data), SHA256: hex.EncodeToString(sum[:])})
		plain.Write(file.Data)
	}
	nonce := make([]byte, sealNonceBytes)
	if _, err := io.ReadFull(random, nonce); err != nil {
		return SealedHostKeys{}, err
	}
	record.NonceHex = hex.EncodeToString(nonce)
	// A placeholder of the right size lets Validate check the header first.
	record.CiphertextHex = strings.Repeat("00", plain.Len()+16)
	if err := record.Validate(); err != nil {
		return SealedHostKeys{}, err
	}
	aead, err := sealAEAD(label, code)
	if err != nil {
		return SealedHostKeys{}, err
	}
	header, err := record.header()
	if err != nil {
		return SealedHostKeys{}, err
	}
	record.CiphertextHex = hex.EncodeToString(aead.Seal(nil, nonce, plain.Bytes(), header))
	return record, nil
}

// OpenHostKeys decrypts a sealed record for the expected host and checks
// every file against its listed size and digest.
func OpenHostKeys(record SealedHostKeys, label string, code []byte) ([]SecretFile, error) {
	if err := record.Validate(); err != nil {
		return nil, err
	}
	if record.Label != label {
		return nil, fmt.Errorf("%w: the record is for host %s, not %s", ErrSeal, record.Label, label)
	}
	aead, err := sealAEAD(label, code)
	if err != nil {
		return nil, err
	}
	header, err := record.header()
	if err != nil {
		return nil, err
	}
	nonce, _ := hex.DecodeString(record.NonceHex)
	sealed, _ := hex.DecodeString(record.CiphertextHex)
	plain, err := aead.Open(nil, nonce, sealed, header)
	if err != nil {
		return nil, fmt.Errorf("%w: the seal code does not open this record", ErrSeal)
	}
	files := make([]SecretFile, 0, len(record.Files))
	offset := 0
	for _, entry := range record.Files {
		data := plain[offset : offset+entry.Bytes]
		offset += entry.Bytes
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != entry.SHA256 {
			clear(plain)
			return nil, fmt.Errorf("%w: %s does not match its digest", ErrSeal, entry.Path)
		}
		files = append(files, SecretFile{Path: entry.Path, Data: data})
	}
	return files, nil
}

// EncodeSealCode is the paper line for a host's seal code: its label, the
// code in sixteen groups of four hex digits, and a check group bound to the
// label.
func EncodeSealCode(label string, code []byte) (string, error) {
	if len(code) != SealCodeBytes || !sealLabel.MatchString(label) {
		return "", ErrSeal
	}
	return encodeCodeLine(sealPaperPrefix, sealCheckDomain, label, code), nil
}

// DecodeSealCode reads a paper line back, ignoring spacing and the case of
// the digits. A mistyped digit or label fails the check group (one in 65,536
// errors would pass it; the record's authentication then catches the rest).
func DecodeSealCode(line string) (string, []byte, error) {
	return decodeCodeLine(line, sealPaperPrefix, sealCheckDomain, SealCodeBytes, sealLabel.MatchString, ErrSeal, "host label")
}
