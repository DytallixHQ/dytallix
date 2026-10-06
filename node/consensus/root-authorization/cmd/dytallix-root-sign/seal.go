package main

import (
	"bufio"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	root "dytallix.local/consensus/root-authorization"
)

// readPaper reads a paper line from a file, or for "-" one line typed on
// standard input (ended by Enter, not by end of input).
func readPaper(path string) ([]byte, error) {
	var raw []byte
	var err error
	if path == "-" {
		var line string
		line, err = bufio.NewReader(io.LimitReader(os.Stdin, maxPaperBytes+1)).ReadString('\n')
		if errors.Is(err, io.EOF) && line != "" {
			err = nil
		}
		raw = []byte(line)
	} else {
		raw, err = readBounded(path, maxPaperBytes)
	}
	if err != nil {
		return nil, err
	}
	if len(raw) > maxPaperBytes {
		clear(raw)
		return nil, errors.New("the paper line is too long")
	}
	return raw, nil
}

// directory requires a clean absolute path to an existing directory that is
// not a symbolic link.
func directory(path string) error {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return fmt.Errorf("%s must be a clean absolute path", path)
	}
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return fmt.Errorf("%s must be an existing directory", path)
	}
	return nil
}

// seal encrypts a host's secret files, read from a staging node home, under
// a fresh seal code. The code is printed for paper and written nowhere.
func seal(label, home, output string, paths []string, out io.Writer) error {
	if err := directory(home); err != nil {
		return err
	}
	files := make([]root.SecretFile, 0, len(paths))
	defer func() {
		for _, file := range files {
			clear(file.Data)
		}
	}()
	for _, path := range paths {
		full := filepath.Join(home, path)
		info, err := os.Lstat(full)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 {
			return fmt.Errorf("%s must be a regular file readable only by its owner", full)
		}
		data, err := readBounded(full, root.MaxSealedFileBytes)
		if err != nil {
			return fmt.Errorf("%s: %w", full, err)
		}
		files = append(files, root.SecretFile{Path: path, Data: data})
	}
	code := make([]byte, root.SealCodeBytes)
	defer clear(code)
	if _, err := rand.Read(code); err != nil {
		return err
	}
	record, err := root.SealHostKeys(label, code, files, rand.Reader)
	if err != nil {
		return err
	}
	if _, err := root.OpenHostKeys(record, label, code); err != nil {
		return err
	}
	line, err := root.EncodeSealCode(label, code)
	if err != nil {
		return err
	}
	if err := writeRecord(output, record, out); err != nil {
		return err
	}
	for _, entry := range record.Files {
		if _, err := fmt.Fprintf(out, "sealed %s sha256 %s\n", entry.Path, entry.SHA256); err != nil {
			return err
		}
	}
	_, err = fmt.Fprintf(out, "seal code for %s (write it on paper twice, then run seal-check):\n%s\n", label, line)
	return err
}

func openSealed(paperPath, sealedPath string) (string, []root.SecretFile, error) {
	var record root.SealedHostKeys
	if err := readRecord(sealedPath, &record); err != nil {
		return "", nil, fmt.Errorf("%s: %w", sealedPath, err)
	}
	raw, err := readPaper(paperPath)
	if err != nil {
		return "", nil, err
	}
	defer clear(raw)
	label, code, err := root.DecodeSealCode(string(raw))
	if err != nil {
		return "", nil, err
	}
	defer clear(code)
	files, err := root.OpenHostKeys(record, label, code)
	return label, files, err
}

// sealCheck checks a paper line against its sealed record without writing
// anything.
func sealCheck(paperPath, sealedPath string, out io.Writer) error {
	label, files, err := openSealed(paperPath, sealedPath)
	if err != nil {
		return err
	}
	for _, file := range files {
		clear(file.Data)
	}
	_, err = fmt.Fprintf(out, "the seal code for %s opens its sealed keys (%d files)\n", label, len(files))
	return err
}

// unseal writes a host's secret files under its node home, owner-only. It
// never replaces a file, and each file's directory must already exist.
func unseal(paperPath, sealedPath, label, outDir string, out io.Writer) error {
	if err := directory(outDir); err != nil {
		return err
	}
	opened, files, err := openSealed(paperPath, sealedPath)
	if err != nil {
		return err
	}
	defer func() {
		for _, file := range files {
			clear(file.Data)
		}
	}()
	if opened != label {
		return fmt.Errorf("the seal code is for host %s, not %s", opened, label)
	}
	for _, file := range files {
		if err := directory(filepath.Join(outDir, filepath.Dir(file.Path))); err != nil {
			return err
		}
	}
	for _, file := range files {
		path := filepath.Join(outDir, file.Path)
		if err := create(path, file.Data, privateKeyFileMod); err != nil {
			return fmt.Errorf("cannot create %s: %w", path, err)
		}
		if _, err := fmt.Fprintf(out, "unsealed %s\n", path); err != nil {
			return err
		}
	}
	return nil
}
