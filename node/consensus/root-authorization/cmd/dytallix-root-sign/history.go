package main

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	root "dytallix.local/consensus/root-authorization"
)

// Metric history copies (monitoring v1, M4b; P01, 10 October 2026): one
// host's finished day of metric history, encrypted under the chain's
// history code, which is separate from the backup code.
const maxHistoryBytes = 64 << 20

// historyCode makes a chain's history code: the code file every host's
// bundle seals, and the paper line, printed once.
func historyCode(chain, output string, out io.Writer) error {
	code := make([]byte, root.HistoryCodeBytes)
	defer clear(code)
	if _, err := rand.Read(code); err != nil {
		return err
	}
	line, err := root.EncodeHistoryCode(chain, code)
	if err != nil {
		return err
	}
	if err := create(output, []byte(line+"\n"), privateKeyFileMod); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "history code for %s (write it on paper twice, then run history-check on a test copy):\n%s\n",
		chain, line)
	return err
}

// readHistoryCodeFile reads a history code file: owner-only, one paper line.
func readHistoryCodeFile(path string) (string, []byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return "", nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 {
		return "", nil, errors.New("the history code file must be a regular file readable only by its owner")
	}
	raw, err := readBounded(path, maxPaperBytes)
	if err != nil {
		return "", nil, err
	}
	defer clear(raw)
	return root.DecodeHistoryCode(string(raw))
}

// historySeal encrypts one day's history file into a new copy file.
func historySeal(codePath, host, day, input, output string, out io.Writer) error {
	chain, code, err := readHistoryCodeFile(codePath)
	if err != nil {
		return err
	}
	defer clear(code)
	source, err := os.Open(input)
	if err != nil {
		return err
	}
	defer source.Close()
	info, err := source.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() > maxHistoryBytes {
		return fmt.Errorf("%s must be a regular file of at most %d bytes", input, maxHistoryBytes)
	}
	partial := output + ".partial"
	file, err := os.OpenFile(partial, os.O_WRONLY|os.O_CREATE|os.O_EXCL, publicFileMode)
	if err != nil {
		return err
	}
	if _, err := os.Lstat(output); err == nil {
		file.Close()
		os.Remove(partial)
		return fmt.Errorf("%s exists; outputs are never overwritten", output)
	}
	digest := sha256.New()
	err = func() error {
		writer, _, err := root.NewHistoryWriter(io.MultiWriter(file, digest), chain, host, day, code, rand.Reader)
		if err != nil {
			return err
		}
		if _, err := io.CopyN(writer, source, info.Size()); err != nil {
			return err
		}
		if err := writer.Close(); err != nil {
			return err
		}
		return file.Sync()
	}()
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Link(partial, output)
	}
	os.Remove(partial)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "wrote %s: chain %s, host %s, day %s, %d bytes, sha256 %s\n",
		output, chain, host, day, info.Size(), hex.EncodeToString(digest.Sum(nil)))
	return err
}

// openHistoryCopy decrypts a whole copy into w; the copy's authenticated
// end is required before it returns.
func openHistoryCopy(paperPath, input string, w io.Writer) (root.HistoryHeader, int64, error) {
	raw, err := readPaper(paperPath)
	if err != nil {
		return root.HistoryHeader{}, 0, err
	}
	chain, code, err := root.DecodeHistoryCode(string(raw))
	clear(raw)
	if err != nil {
		return root.HistoryHeader{}, 0, err
	}
	defer clear(code)
	file, err := os.Open(input)
	if err != nil {
		return root.HistoryHeader{}, 0, err
	}
	defer file.Close()
	reader, header, err := root.NewHistoryReader(file, code)
	if err != nil {
		return header, 0, err
	}
	if header.ChainID != chain {
		return header, 0, fmt.Errorf("the history code is for %s, the copy for %s", chain, header.ChainID)
	}
	size, err := io.Copy(w, io.LimitReader(reader, maxHistoryBytes+1))
	if err == nil && size > maxHistoryBytes {
		err = fmt.Errorf("the copy holds more than %d bytes", maxHistoryBytes)
	}
	return header, size, err
}

// historyCheck checks that a paper line opens a copy, writing nothing.
func historyCheck(paperPath, input string, out io.Writer) error {
	header, size, err := openHistoryCopy(paperPath, input, io.Discard)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "the history code opens %s: chain %s, host %s, day %s, %d bytes\n",
		input, header.ChainID, header.Host, header.Day, size)
	return err
}

// historyOpen decrypts a copy into a new owner-only file; nothing is left
// at the output unless the whole copy opens.
func historyOpen(paperPath, input, output string, out io.Writer) error {
	if err := directory(filepath.Dir(output)); err != nil {
		return err
	}
	if _, err := os.Lstat(output); err == nil {
		return fmt.Errorf("%s exists; outputs are never overwritten", output)
	}
	partial := output + ".partial"
	file, err := os.OpenFile(partial, os.O_WRONLY|os.O_CREATE|os.O_EXCL, privateKeyFileMod)
	if err != nil {
		return err
	}
	header, size, err := openHistoryCopy(paperPath, input, file)
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Link(partial, output)
	}
	os.Remove(partial)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "opened %s into %s: chain %s, host %s, day %s, %d bytes\n",
		input, output, header.ChainID, header.Host, header.Day, size)
	return err
}
