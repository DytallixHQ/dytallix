package rootauthorization

import (
	"bufio"
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
)

// Off-host backups (disaster recovery v1, F19; P01, 7 October 2026). The
// sentry encrypts its newest snapshot under the chain's backup code (32
// random bytes, written on paper as a checked line naming the chain, like
// the seal codes) and uploads the copy to storage at a second provider.
//
// A copy is a header line (public facts: the chain, the height, a random
// salt and the chunk size) followed by records: a 4-byte big-endian length
// and an AES-256-GCM chunk of at most BackupChunkBytes of plaintext. The key
// is SHAKE256(backupDomain || len(chain) || chain || height || salt || code),
// so each copy has its own key and the chunk index is a safe nonce. Each
// chunk's additional data is the header's SHA-256, its index and a final
// flag: a reordered, dropped, truncated, extended or altered copy is
// refused, and the last record is always a final chunk, possibly empty.
const (
	BackupSchema      = "dytallix.backup.v1"
	BackupCodeBytes   = 32
	BackupChunkBytes  = 1 << 20
	MaxBackupChunks   = 1 << 24
	backupDomain      = "DYTALLIX/BACKUP/v1\x00"
	backupCheckDomain = "DYTALLIX/BACKUP-CHECK/v1\x00"
	backupPaperPrefix = "dytallix-backup-"
	backupSaltBytes   = 32
	maxBackupHeader   = 4096
)

var ErrBackup = errors.New("invalid backup")

// backupChain is a chain ID as the genesis builder accepts it.
var backupChain = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,62}$`)

// BackupHeader is a copy's first line.
type BackupHeader struct {
	Schema     string `json:"schema"`
	ChainID    string `json:"chain_id"`
	Height     uint64 `json:"height"`
	SaltHex    string `json:"salt_hex"`
	ChunkBytes int    `json:"chunk_bytes"`
}

func (h BackupHeader) validate() error {
	if h.Schema != BackupSchema || !backupChain.MatchString(h.ChainID) || h.Height == 0 ||
		h.ChunkBytes != BackupChunkBytes || !hexDigest(h.SaltHex, backupSaltBytes) {
		return ErrBackup
	}
	return nil
}

// EncodeBackupCode is the paper line for a chain's backup code.
func EncodeBackupCode(chain string, code []byte) (string, error) {
	if len(code) != BackupCodeBytes || !backupChain.MatchString(chain) {
		return "", ErrBackup
	}
	return encodeCodeLine(backupPaperPrefix, backupCheckDomain, chain, code), nil
}

// DecodeBackupCode reads a backup code's paper line back: its chain and code.
func DecodeBackupCode(line string) (string, []byte, error) {
	return decodeCodeLine(line, backupPaperPrefix, backupCheckDomain, BackupCodeBytes, backupChain.MatchString, ErrBackup, "chain")
}

type backupCipher struct {
	aead   cipher.AEAD
	header [sha256.Size]byte
}

func newBackupCipher(header BackupHeader, raw []byte, code []byte) (*backupCipher, error) {
	if len(code) != BackupCodeBytes {
		return nil, ErrBackup
	}
	salt, _ := hex.DecodeString(header.SaltHex)
	height := make([]byte, 8)
	binary.BigEndian.PutUint64(height, header.Height)
	key := make([]byte, 32)
	defer clear(key)
	shake(key, []byte(backupDomain), []byte{byte(len(header.ChainID))}, []byte(header.ChainID), height, salt, code)
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return &backupCipher{aead: aead, header: sha256.Sum256(raw)}, nil
}

func (c *backupCipher) nonceAndData(index uint64, final bool) ([]byte, []byte) {
	nonce := make([]byte, c.aead.NonceSize())
	binary.BigEndian.PutUint64(nonce[len(nonce)-8:], index)
	data := make([]byte, 0, sha256.Size+9)
	data = append(data, c.header[:]...)
	data = binary.BigEndian.AppendUint64(data, index)
	if final {
		data = append(data, 1)
	} else {
		data = append(data, 0)
	}
	return nonce, data
}

// BackupWriter encrypts a copy as it is written. Close writes the final
// chunk; a copy without it is refused.
type BackupWriter struct {
	out    io.Writer
	cipher *backupCipher
	buffer []byte
	index  uint64
	closed bool
}

// NewBackupWriter writes the header of a new copy of chain at height.
func NewBackupWriter(out io.Writer, chain string, height uint64, code []byte, random io.Reader) (*BackupWriter, BackupHeader, error) {
	salt := make([]byte, backupSaltBytes)
	if _, err := io.ReadFull(random, salt); err != nil {
		return nil, BackupHeader{}, err
	}
	header := BackupHeader{Schema: BackupSchema, ChainID: chain, Height: height,
		SaltHex: hex.EncodeToString(salt), ChunkBytes: BackupChunkBytes}
	if err := header.validate(); err != nil {
		return nil, BackupHeader{}, err
	}
	raw, err := json.Marshal(header)
	if err != nil {
		return nil, BackupHeader{}, err
	}
	c, err := newBackupCipher(header, raw, code)
	if err != nil {
		return nil, BackupHeader{}, err
	}
	if _, err := out.Write(append(raw, '\n')); err != nil {
		return nil, BackupHeader{}, err
	}
	return &BackupWriter{out: out, cipher: c, buffer: make([]byte, 0, BackupChunkBytes)}, header, nil
}

func (w *BackupWriter) seal(final bool) error {
	if w.index >= MaxBackupChunks {
		return fmt.Errorf("%w: the copy exceeds %d chunks", ErrBackup, MaxBackupChunks)
	}
	nonce, data := w.cipher.nonceAndData(w.index, final)
	sealed := w.cipher.aead.Seal(nil, nonce, w.buffer, data)
	clear(w.buffer)
	w.buffer = w.buffer[:0]
	w.index++
	length := binary.BigEndian.AppendUint32(nil, uint32(len(sealed)))
	if _, err := w.out.Write(length); err != nil {
		return err
	}
	_, err := w.out.Write(sealed)
	return err
}

func (w *BackupWriter) Write(p []byte) (int, error) {
	if w.closed {
		return 0, ErrBackup
	}
	written := 0
	for len(p) > 0 {
		take := min(len(p), BackupChunkBytes-len(w.buffer))
		w.buffer = append(w.buffer, p[:take]...)
		p, written = p[take:], written+take
		if len(w.buffer) == BackupChunkBytes {
			if err := w.seal(false); err != nil {
				return written, err
			}
		}
	}
	return written, nil
}

// Close seals the final chunk, possibly empty.
func (w *BackupWriter) Close() error {
	if w.closed {
		return nil
	}
	w.closed = true
	return w.seal(true)
}

// BackupReader decrypts a copy as it is read. It returns io.EOF only after
// an authenticated final chunk and the end of the input.
type BackupReader struct {
	in     *bufio.Reader
	cipher *backupCipher
	plain  []byte
	index  uint64
	done   bool
}

// NewBackupReader reads a copy's header and prepares to decrypt it.
func NewBackupReader(in io.Reader, code []byte) (*BackupReader, BackupHeader, error) {
	reader := bufio.NewReaderSize(in, maxBackupHeader)
	line, err := reader.ReadSlice('\n')
	if err != nil {
		return nil, BackupHeader{}, fmt.Errorf("%w: no header", ErrBackup)
	}
	raw := bytes.TrimSuffix(line, []byte("\n"))
	var header BackupHeader
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&header); err != nil || header.validate() != nil {
		return nil, BackupHeader{}, fmt.Errorf("%w: header", ErrBackup)
	}
	canonical, err := json.Marshal(header)
	if err != nil || !bytes.Equal(canonical, raw) {
		return nil, BackupHeader{}, fmt.Errorf("%w: header is not canonical", ErrBackup)
	}
	c, err := newBackupCipher(header, raw, code)
	if err != nil {
		return nil, BackupHeader{}, err
	}
	return &BackupReader{in: reader, cipher: c}, header, nil
}

func (r *BackupReader) next() error {
	var length [4]byte
	if _, err := io.ReadFull(r.in, length[:]); err != nil {
		return fmt.Errorf("%w: the copy ends before its final chunk", ErrBackup)
	}
	size := binary.BigEndian.Uint32(length[:])
	overhead := uint32(r.cipher.aead.Overhead())
	if size < overhead || size > BackupChunkBytes+overhead || r.index >= MaxBackupChunks {
		return fmt.Errorf("%w: chunk %d size", ErrBackup, r.index)
	}
	sealed := make([]byte, size)
	if _, err := io.ReadFull(r.in, sealed); err != nil {
		return fmt.Errorf("%w: the copy ends inside chunk %d", ErrBackup, r.index)
	}
	for _, final := range []bool{false, true} {
		nonce, data := r.cipher.nonceAndData(r.index, final)
		if plain, err := r.cipher.aead.Open(nil, nonce, sealed, data); err == nil {
			if !final && len(plain) != BackupChunkBytes {
				return fmt.Errorf("%w: chunk %d is short", ErrBackup, r.index)
			}
			r.plain, r.done = plain, final
			r.index++
			if final {
				if _, err := r.in.ReadByte(); err != io.EOF {
					return fmt.Errorf("%w: data after the final chunk", ErrBackup)
				}
			}
			return nil
		}
	}
	return fmt.Errorf("%w: chunk %d does not open with this code (wrong code, or altered, reordered or truncated copy)", ErrBackup, r.index)
}

func (r *BackupReader) Read(p []byte) (int, error) {
	for len(r.plain) == 0 {
		if r.done {
			return 0, io.EOF
		}
		if err := r.next(); err != nil {
			return 0, err
		}
	}
	n := copy(p, r.plain)
	r.plain = r.plain[n:]
	return n, nil
}
