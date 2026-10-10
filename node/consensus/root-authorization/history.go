package rootauthorization

import (
	"encoding/hex"
	"encoding/json"
	"io"
	"time"
)

// Metric history copies (monitoring v1, M4b; P01, 10 October 2026). Each
// host's monitor job encrypts a finished day's history file under the
// chain's history code and uploads it, so the founder can chart it on their
// own machine. The history code is separate from the backup code, which
// stays on the sentry: it is sealed on every host and opens only history.
//
// A history copy is the backup stream (BackupWriter and BackupReader:
// AES-256-GCM chunks bound to the header line) under its own header and
// key: SHAKE256(historyDomain || len(chain) || chain || len(host) || host ||
// day || salt || code). The header names the chain, the host and the UTC
// day, so a copy cannot pass for another host's or another day's.
const (
	HistorySchema      = "dytallix.history.v1"
	HistoryCodeBytes   = 32
	historyDomain      = "DYTALLIX/HISTORY/v1\x00"
	historyCheckDomain = "DYTALLIX/HISTORY-CHECK/v1\x00"
	historyPaperPrefix = "dytallix-history-"
	historyDay         = "2006-01-02"
)

// HistoryHeader is a history copy's first line.
type HistoryHeader struct {
	Schema     string `json:"schema"`
	ChainID    string `json:"chain_id"`
	Host       string `json:"host"`
	Day        string `json:"day"`
	SaltHex    string `json:"salt_hex"`
	ChunkBytes int    `json:"chunk_bytes"`
}

func (h HistoryHeader) validate() error {
	day, err := time.Parse(historyDay, h.Day)
	if h.Schema != HistorySchema || !backupChain.MatchString(h.ChainID) || !sealLabel.MatchString(h.Host) ||
		err != nil || day.Format(historyDay) != h.Day || h.ChunkBytes != BackupChunkBytes ||
		!hexDigest(h.SaltHex, backupSaltBytes) {
		return ErrBackup
	}
	return nil
}

// EncodeHistoryCode is the paper line for a chain's history code.
func EncodeHistoryCode(chain string, code []byte) (string, error) {
	if len(code) != HistoryCodeBytes || !backupChain.MatchString(chain) {
		return "", ErrBackup
	}
	return encodeCodeLine(historyPaperPrefix, historyCheckDomain, chain, code), nil
}

// DecodeHistoryCode reads a history code's paper line back: its chain and code.
func DecodeHistoryCode(line string) (string, []byte, error) {
	return decodeCodeLine(line, historyPaperPrefix, historyCheckDomain, HistoryCodeBytes, backupChain.MatchString,
		ErrBackup, "chain")
}

func newHistoryCipher(header HistoryHeader, raw []byte, code []byte) (*backupCipher, error) {
	if len(code) != HistoryCodeBytes {
		return nil, ErrBackup
	}
	salt, _ := hex.DecodeString(header.SaltHex)
	return newChunkCipher(raw, []byte(historyDomain), []byte{byte(len(header.ChainID))}, []byte(header.ChainID),
		[]byte{byte(len(header.Host))}, []byte(header.Host), []byte(header.Day), salt, code)
}

// NewHistoryWriter writes the header of a new history copy of host's day.
// Close writes the final chunk.
func NewHistoryWriter(out io.Writer, chain, host, day string, code []byte, random io.Reader) (*BackupWriter, HistoryHeader, error) {
	salt := make([]byte, backupSaltBytes)
	if _, err := io.ReadFull(random, salt); err != nil {
		return nil, HistoryHeader{}, err
	}
	header := HistoryHeader{Schema: HistorySchema, ChainID: chain, Host: host, Day: day,
		SaltHex: hex.EncodeToString(salt), ChunkBytes: BackupChunkBytes}
	if err := header.validate(); err != nil {
		return nil, HistoryHeader{}, err
	}
	raw, err := json.Marshal(header)
	if err != nil {
		return nil, HistoryHeader{}, err
	}
	c, err := newHistoryCipher(header, raw, code)
	if err != nil {
		return nil, HistoryHeader{}, err
	}
	writer, err := newChunkWriter(out, raw, c)
	if err != nil {
		return nil, HistoryHeader{}, err
	}
	return writer, header, nil
}

// NewHistoryReader reads a history copy's header and prepares to decrypt it.
func NewHistoryReader(in io.Reader, code []byte) (*BackupReader, HistoryHeader, error) {
	var header HistoryHeader
	reader, raw, err := readHeaderLine(in, &header)
	if err != nil {
		return nil, HistoryHeader{}, err
	}
	c, err := newHistoryCipher(header, raw, code)
	if err != nil {
		return nil, HistoryHeader{}, err
	}
	return &BackupReader{in: reader, cipher: c}, header, nil
}
