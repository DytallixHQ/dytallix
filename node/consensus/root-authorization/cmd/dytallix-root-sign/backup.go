package main

import (
	"archive/tar"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strconv"
	"time"

	root "dytallix.local/consensus/root-authorization"
)

// Off-host backups (disaster recovery v1, F19; P01, 7 October 2026): a
// snapshot directory and the light blocks its restore verifies, packed as a
// deterministic tar and encrypted under the chain's backup code.
const maxBackupEntries = 1 << 20

// lightBlocksEntry is the copy's directory for the snapshot's light blocks:
// heights H to H+2, a block and a parameters file each (R4).
const lightBlocksEntry = "light-blocks"

// checkLightBlocks requires exactly the six light block files of heights
// height to height+2 in dir.
func checkLightBlocks(dir string, height uint64) error {
	if err := directory(dir); err != nil {
		return err
	}
	want := map[string]bool{}
	for h := height; h <= height+2; h++ {
		want[fmt.Sprintf("%020d.block", h)] = true
		want[fmt.Sprintf("%020d.params", h)] = true
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if !want[entry.Name()] || !entry.Type().IsRegular() {
			return fmt.Errorf("%s: %s is not a light block file of heights %d to %d", dir, entry.Name(), height, height+2)
		}
		delete(want, entry.Name())
	}
	if len(want) != 0 {
		return fmt.Errorf("%s lacks light block files of heights %d to %d", dir, height, height+2)
	}
	return nil
}

// backupCode makes a chain's backup code: the code file the sentry's bundle
// seals, and the paper line, printed once.
func backupCode(chain, output string, out io.Writer) error {
	code := make([]byte, root.BackupCodeBytes)
	defer clear(code)
	if _, err := rand.Read(code); err != nil {
		return err
	}
	line, err := root.EncodeBackupCode(chain, code)
	if err != nil {
		return err
	}
	if err := create(output, []byte(line+"\n"), privateKeyFileMod); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "backup code for %s (write it on paper twice, then run backup-check on a test copy):\n%s\n", chain, line)
	return err
}

// readCodeFile reads a backup code file: owner-only, one paper line.
func readCodeFile(path string) (string, []byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return "", nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 {
		return "", nil, errors.New("the backup code file must be a regular file readable only by its owner")
	}
	raw, err := readBounded(path, maxPaperBytes)
	if err != nil {
		return "", nil, err
	}
	defer clear(raw)
	return root.DecodeBackupCode(string(raw))
}

// packDirectory writes dir as a tar, with lightBlocks under light-blocks/:
// sorted, root-owned, time zero, only directories and regular files.
func packDirectory(dir, lightBlocks string, w io.Writer) (int, error) {
	if err := directory(dir); err != nil {
		return 0, err
	}
	if _, err := os.Lstat(filepath.Join(dir, lightBlocksEntry)); err == nil {
		return 0, fmt.Errorf("%s already has a %s entry", dir, lightBlocksEntry)
	}
	archive := tar.NewWriter(w)
	entries := 0
	if err := packTree(archive, dir, "", &entries); err != nil {
		return entries, err
	}
	if err := packTree(archive, lightBlocks, lightBlocksEntry, &entries); err != nil {
		return entries, err
	}
	return entries, archive.Close()
}

// packTree adds dir's entries to archive, under prefix when it is not empty.
func packTree(archive *tar.Writer, dir, prefix string, entries *int) error {
	return filepath.WalkDir(dir, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || (path == dir && prefix == "") {
			return err
		}
		relative, err := filepath.Rel(dir, path)
		if err != nil {
			return err
		}
		if prefix != "" {
			relative = filepath.Join(prefix, relative)
			if path == dir {
				relative = prefix
			}
		}
		if *entries++; *entries > maxBackupEntries {
			return errors.New("the snapshot has too many entries")
		}
		header := &tar.Header{Name: filepath.ToSlash(relative), ModTime: time.Unix(0, 0), Format: tar.FormatPAX}
		switch {
		case entry.IsDir():
			header.Typeflag, header.Name, header.Mode = tar.TypeDir, header.Name+"/", 0o755
			return archive.WriteHeader(header)
		case entry.Type().IsRegular():
			file, err := os.Open(path)
			if err != nil {
				return err
			}
			defer file.Close()
			info, err := file.Stat()
			if err != nil {
				return err
			}
			header.Typeflag, header.Mode, header.Size = tar.TypeReg, 0o644, info.Size()
			if err := archive.WriteHeader(header); err != nil {
				return err
			}
			_, err = io.CopyN(archive, file, info.Size())
			return err
		default:
			return fmt.Errorf("%s is not a directory or a regular file", path)
		}
	})
}

// backupSeal encrypts a snapshot directory and its light blocks into a new
// copy file.
func backupSeal(codePath, heightValue, dir, lightBlocks, output string, out io.Writer) error {
	chain, code, err := readCodeFile(codePath)
	if err != nil {
		return err
	}
	defer clear(code)
	height, err := strconv.ParseUint(heightValue, 10, 64)
	if err != nil {
		return errors.New("the height must be a positive integer")
	}
	if lightBlocks == "" {
		return errors.New("-light-blocks is required: a copy carries the light blocks its restore verifies")
	}
	if err := checkLightBlocks(lightBlocks, height); err != nil {
		return err
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
	entries, err := func() (int, error) {
		writer, _, err := root.NewBackupWriter(io.MultiWriter(file, digest), chain, height, code, rand.Reader)
		if err != nil {
			return 0, err
		}
		entries, err := packDirectory(dir, lightBlocks, writer)
		if err != nil {
			return entries, err
		}
		if err := writer.Close(); err != nil {
			return entries, err
		}
		return entries, file.Sync()
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
	_, err = fmt.Fprintf(out, "wrote %s: chain %s, height %d, %d entries, sha256 %s\n",
		output, chain, height, entries, hex.EncodeToString(digest.Sum(nil)))
	return err
}

// readBackupPaper reads a backup code's paper line, typed or from a file.
func readBackupPaper(paperPath string) (string, []byte, error) {
	raw, err := readPaper(paperPath)
	if err != nil {
		return "", nil, err
	}
	defer clear(raw)
	return root.DecodeBackupCode(string(raw))
}

// openCopy opens a copy and walks its tar: every entry is checked, and
// visit sees each one.
func openCopy(chain string, code []byte, input string, visit func(*tar.Header, io.Reader) error) (root.BackupHeader, int, error) {
	file, err := os.Open(input)
	if err != nil {
		return root.BackupHeader{}, 0, err
	}
	defer file.Close()
	reader, header, err := root.NewBackupReader(file, code)
	if err != nil {
		return header, 0, err
	}
	if header.ChainID != chain {
		return header, 0, fmt.Errorf("the backup code is for %s, the copy for %s", chain, header.ChainID)
	}
	archive := tar.NewReader(reader)
	entries := 0
	for {
		entry, err := archive.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return header, entries, err
		}
		if entries++; entries > maxBackupEntries {
			return header, entries, errors.New("the copy has too many entries")
		}
		name := filepath.FromSlash(entry.Name)
		if !filepath.IsLocal(name) || (entry.Typeflag != tar.TypeDir && entry.Typeflag != tar.TypeReg) {
			return header, entries, fmt.Errorf("unsafe entry %q", entry.Name)
		}
		if err := visit(entry, archive); err != nil {
			return header, entries, err
		}
	}
	// The tar's end must also be the copy's authenticated end.
	if _, err := io.Copy(io.Discard, reader); err != nil {
		return header, entries, err
	}
	return header, entries, nil
}

// backupCheck checks that a paper line opens a copy and that every entry
// is safe, writing nothing.
func backupCheck(paperPath, input string, out io.Writer) error {
	chain, code, err := readBackupPaper(paperPath)
	if err != nil {
		return err
	}
	defer clear(code)
	header, entries, err := checkCopy(chain, code, input)
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "the backup code opens %s: chain %s, height %d, %d entries\n",
		input, header.ChainID, header.Height, entries)
	return err
}

func checkCopy(chain string, code []byte, input string) (root.BackupHeader, int, error) {
	return openCopy(chain, code, input, func(_ *tar.Header, r io.Reader) error {
		_, err := io.Copy(io.Discard, r)
		return err
	})
}

// backupOpen checks a copy completely, then extracts it into a new
// directory, owner-only.
func backupOpen(paperPath, input, output string, out io.Writer) error {
	if !filepath.IsAbs(output) || filepath.Clean(output) != output {
		return fmt.Errorf("%s must be a clean absolute path", output)
	}
	if err := directory(filepath.Dir(output)); err != nil {
		return err
	}
	chain, code, err := readBackupPaper(paperPath)
	if err != nil {
		return err
	}
	defer clear(code)
	if _, _, err := checkCopy(chain, code, input); err != nil {
		return err
	}
	if err := os.Mkdir(output, 0o700); err != nil {
		return err
	}
	header, entries, err := openCopy(chain, code, input, func(entry *tar.Header, r io.Reader) error {
		target := filepath.Join(output, filepath.FromSlash(entry.Name))
		if entry.Typeflag == tar.TypeDir {
			return os.Mkdir(target, 0o700)
		}
		file, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, privateKeyFileMod)
		if err != nil {
			return err
		}
		_, err = io.Copy(file, r)
		if closeErr := file.Close(); err == nil {
			err = closeErr
		}
		return err
	})
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "opened %s into %s: chain %s, height %d, %d entries\n",
		input, output, header.ChainID, header.Height, entries)
	return err
}
