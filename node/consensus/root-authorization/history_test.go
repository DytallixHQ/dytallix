package rootauthorization

import (
	"bytes"
	"crypto/rand"
	"errors"
	"io"
	"strings"
	"testing"
)

func testHistory(t *testing.T, plain []byte, code []byte, host, day string) []byte {
	t.Helper()
	var out bytes.Buffer
	writer, header, err := NewHistoryWriter(&out, "dytallix-staging-1", host, day, code, rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	if header.Host != host || header.Day != day || header.Schema != HistorySchema {
		t.Fatalf("header %+v", header)
	}
	if _, err := writer.Write(plain); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

func openHistory(copyBytes []byte, code []byte) ([]byte, HistoryHeader, error) {
	reader, header, err := NewHistoryReader(bytes.NewReader(copyBytes), code)
	if err != nil {
		return nil, header, err
	}
	plain, err := io.ReadAll(reader)
	return plain, header, err
}

func TestHistoryRoundTrips(t *testing.T) {
	code := testCode(11)
	plain := bytes.Repeat([]byte(`{"t":1800000000,"h":12}`+"\n"), 90_000) // past one chunk
	copyBytes := testHistory(t, plain, code, "validator-1", "2026-10-10")
	opened, header, err := openHistory(copyBytes, code)
	if err != nil || !bytes.Equal(opened, plain) {
		t.Fatalf("round trip: %v", err)
	}
	if header.ChainID != "dytallix-staging-1" || header.Host != "validator-1" || header.Day != "2026-10-10" {
		t.Fatalf("header %+v", header)
	}
	if _, _, err := openHistory(copyBytes, testCode(12)); !errors.Is(err, ErrBackup) {
		t.Fatalf("another code opened the copy: %v", err)
	}
	// An empty day still has its final chunk.
	if opened, _, err := openHistory(testHistory(t, nil, code, "sentry-1", "2026-10-11"), code); err != nil || len(opened) != 0 {
		t.Fatalf("empty copy: %v", err)
	}
}

func TestHistoryBindsHostAndDay(t *testing.T) {
	code := testCode(11)
	copyBytes := testHistory(t, []byte("history"), code, "validator-1", "2026-10-10")
	for _, swap := range [][2]string{{`"host":"validator-1"`, `"host":"endpoint-1"`},
		{`"day":"2026-10-10"`, `"day":"2026-10-09"`}} {
		altered := bytes.Replace(copyBytes, []byte(swap[0]), []byte(swap[1]), 1)
		if bytes.Equal(altered, copyBytes) {
			t.Fatalf("%s not found", swap[0])
		}
		if _, _, err := openHistory(altered, code); !errors.Is(err, ErrBackup) {
			t.Fatalf("a copy relabelled %s opened: %v", swap[1], err)
		}
	}
}

func TestHistoryAndBackupCopiesAreDistinct(t *testing.T) {
	code := testCode(11)
	backup := testBackup(t, []byte("snapshot"), code)
	if _, _, err := openHistory(backup, code); !errors.Is(err, ErrBackup) {
		t.Fatalf("a backup copy opened as history: %v", err)
	}
	history := testHistory(t, []byte("history"), code, "validator-1", "2026-10-10")
	if _, err := openBackup(history, code); !errors.Is(err, ErrBackup) {
		t.Fatalf("a history copy opened as a backup: %v", err)
	}
}

func TestHistoryRefusesBadHeaders(t *testing.T) {
	code := testCode(11)
	for _, c := range []struct{ host, day string }{{"", "2026-10-10"}, {"../x", "2026-10-10"},
		{"validator-1", "2026-13-01"}, {"validator-1", "2026-10-1"}, {"validator-1", "10/10/2026"}} {
		if _, _, err := NewHistoryWriter(io.Discard, "dytallix-staging-1", c.host, c.day, code, rand.Reader); err == nil {
			t.Fatalf("accepted host %q day %q", c.host, c.day)
		}
	}
	if _, _, err := NewHistoryWriter(io.Discard, "dytallix-staging-1", "validator-1", "2026-10-10", code[:31], rand.Reader); err == nil {
		t.Fatal("accepted a short code")
	}
}

func TestHistoryCodeLine(t *testing.T) {
	code := testCode(13)
	line, err := EncodeHistoryCode("dytallix-mainnet-1", code)
	if err != nil || !strings.HasPrefix(line, "dytallix-history-dytallix-mainnet-1 ") || len(strings.Fields(line)) != 18 {
		t.Fatalf("line %q: %v", line, err)
	}
	if _, _, err := DecodeHistoryCode(strings.Join(strings.Fields(line)[1:], " ")); err == nil {
		t.Fatal("a line without its prefix decoded")
	}
	// Spacing and the case of the digits do not matter.
	chain, decoded, err := DecodeHistoryCode(line[:36] + strings.ToUpper(line[36:]))
	if err != nil || chain != "dytallix-mainnet-1" || !bytes.Equal(decoded, code) {
		t.Fatalf("decode: %v", err)
	}
	// The backup code's line for the same bytes is not a history code.
	backupLine, _ := EncodeBackupCode("dytallix-mainnet-1", code)
	if _, _, err := DecodeHistoryCode(backupLine); err == nil {
		t.Fatal("a backup code line decoded as a history code")
	}
	fields := strings.Fields(line)
	fields[3] = "ffff"
	if _, _, err := DecodeHistoryCode(strings.Join(fields, " ")); !errors.Is(err, ErrBackup) {
		t.Fatalf("a mistyped group passed: %v", err)
	}
}
