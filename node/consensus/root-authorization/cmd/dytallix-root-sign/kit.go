package main

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"path/filepath"
	"strconv"

	root "dytallix.local/consensus/root-authorization"
)

const maxPaperBytes = 512

func kitName(number int, purpose, suffix string) string {
	return fmt.Sprintf("kit-%d-%s%s", number, purpose, suffix)
}

// kit makes a new kit: a fresh 32-byte secret, its paper line and the four
// private keys on the kit drive, and the four public key records.
func kit(number int, privateDir, publicDir string, out io.Writer) error {
	secret := make([]byte, root.KitSecretBytes)
	defer clear(secret)
	if _, err := rand.Read(secret); err != nil {
		return err
	}
	line, err := root.EncodeKitSecret(number, secret)
	if err != nil {
		return err
	}
	if err := writeKit(number, secret, privateDir, publicDir, out); err != nil {
		return err
	}
	if err := create(filepath.Join(privateDir, fmt.Sprintf("kit-%d-paper.txt", number)), []byte(line+"\n"), privateKeyFileMod); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "kit %d paper line (write it down exactly, then run kit-check):\n%s\n", number, line)
	return err
}

// writeKit derives the kit's keys and writes them; with no public directory
// it writes only the private keys.
func writeKit(number int, secret []byte, privateDir, publicDir string, out io.Writer) error {
	for _, purpose := range root.KitPurposes {
		public, private, err := root.DeriveKitKey(secret, number, purpose)
		if err != nil {
			return err
		}
		// Derivation makes a consistent pair; a signature check per key would
		// cost seconds each on the ceremony machine. The paper check and the
		// proofs of possession sign with every key later.
		err = create(filepath.Join(privateDir, kitName(number, purpose, ".key")), private, privateKeyFileMod)
		clear(private)
		if err != nil {
			return err
		}
		if publicDir != "" {
			record := root.AuthorityKey{KeyID: root.KeyID(public), PublicKeyHex: hex.EncodeToString(public)}
			if err := writeRecord(filepath.Join(publicDir, kitName(number, purpose, ".json")), record, out); err != nil {
				return err
			}
		}
		if _, err := fmt.Fprintf(out, "kit %d %s key_id %s\n", number, purpose, root.KeyID(public)); err != nil {
			return err
		}
	}
	return nil
}

// kitCheck reads a paper line and checks that it derives exactly the four
// published keys. With a private directory it then writes the four private
// keys there, restoring a lost or damaged kit drive.
func kitCheck(paperPath, publicDir, privateDir string, out io.Writer) error {
	raw, err := readPaper(paperPath)
	if err != nil {
		return err
	}
	defer clear(raw)
	number, secret, err := root.DecodeKitSecret(string(raw))
	if err != nil {
		return err
	}
	defer clear(secret)
	for _, purpose := range root.KitPurposes {
		public, private, err := root.DeriveKitKey(secret, number, purpose)
		clear(private)
		if err != nil {
			return err
		}
		var record root.AuthorityKey
		if err := readRecord(filepath.Join(publicDir, kitName(number, purpose, ".json")), &record); err != nil {
			return err
		}
		if record.KeyID != root.KeyID(public) || record.PublicKeyHex != hex.EncodeToString(public) {
			return fmt.Errorf("kit %d %s: the paper line does not derive the published key", number, purpose)
		}
	}
	if _, err := fmt.Fprintf(out, "kit %d paper line matches its four public keys\n", number); err != nil {
		return err
	}
	if privateDir == "" {
		return nil
	}
	return writeKit(number, secret, privateDir, "", out)
}

func prove(keyPath, publicPath, chain, controller, purpose, epochValue, session, output string, out io.Writer) error {
	var record root.AuthorityKey
	if err := readRecord(publicPath, &record); err != nil {
		return err
	}
	var epoch *uint64
	if epochValue != "none" {
		value, err := strconv.ParseUint(epochValue, 10, 64)
		if err != nil || strconv.FormatUint(value, 10) != epochValue {
			return errors.New("the epoch is a positive integer, or none for genesis")
		}
		epoch = &value
	}
	private, err := readPrivateKey(keyPath)
	if err != nil {
		return err
	}
	defer clear(private)
	public, err := hex.DecodeString(record.PublicKeyHex)
	if err != nil || root.KeyID(public) != record.KeyID {
		return errors.New("the public key record is inconsistent")
	}
	// ProvePossession refuses a private key whose public key is not the record's.
	proof, err := root.ProvePossession(root.PossessionChallenge{Schema: root.PossessionSchema, ChainID: chain, ControllerID: controller,
		Purpose: purpose, AuthorityEpoch: epoch, KeyID: record.KeyID, Session: session}, private)
	if err != nil {
		return err
	}
	return writeRecord(output, proof, out)
}

func verifyProof(path string, out io.Writer) error {
	raw, err := readBounded(path, maxRecordBytes)
	if err != nil {
		return err
	}
	var proof root.PossessionProof
	if err := root.DecodeCanonical(raw, &proof); err != nil {
		return err
	}
	if err := root.VerifyPossession(proof); err != nil {
		return err
	}
	epoch := "none"
	if proof.Challenge.AuthorityEpoch != nil {
		epoch = strconv.FormatUint(*proof.Challenge.AuthorityEpoch, 10)
	}
	_, err = fmt.Fprintf(out, "verified %s key_id %s chain %s controller %s epoch %s session %s\n",
		proof.Challenge.Purpose, proof.Challenge.KeyID, proof.Challenge.ChainID, proof.Challenge.ControllerID, epoch, proof.Challenge.Session)
	return err
}
