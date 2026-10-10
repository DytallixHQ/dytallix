// Command dytallix-root-sign is the offline signer for the root genesis
// signers (production activation v1, step A2). Each signer runs it on their
// own device: the private key is read from a local file and never leaves it.
//
//	keygen  -private-key-out PATH -public-key-out PATH
//	policy  -chain-id ID -out PATH KEY.json x5
//	digest  -native-genesis F -config F -engine-genesis F -release-manifest F
//	sign    -policy F -private-key F -bundle-sha512 HEX -out PATH
//	combine -policy F -out PATH SIGNATURES.json...
//	verify  -policy F -signatures F -bundle-sha512 HEX
//
// The key ceremony (launch/custody/KEY_CEREMONY.md) uses the kit commands:
//
//	kit          -number N -private-out DIR -public-out DIR
//	kit-check    -paper F -public DIR
//	kit-restore  -paper F -public DIR -private-out DIR
//	prove        -private-key F -public-key F -chain-id ID -controller ID
//	             -purpose P -epoch N|none -session ID -out PATH
//	verify-proof -proof F
//
// Root controls (node/docs/mainnet/control-signing.md) are signed offline,
// one key at a time, from the request dytallix-control prepares:
//
//	show-control   -request F
//	sign-control   -request F -private-key F -public-key F -operation OP -sequence N -out PATH
//	verify-control -request F -signature F -public-key F
//
// Host bundles (node/docs/architecture/host-setup-v1.md) carry each host's
// node keys sealed under a seal code that is written only on paper:
//
//	seal       -label L -home DIR -out PATH FILE...
//	seal-check -paper F -sealed F
//	unseal     -paper F -sealed F -label L -out DIR
//
// Off-host backups (node/docs/architecture/disaster-recovery-v1.md) encrypt
// a snapshot directory under the chain's backup code:
//
//	backup-code  -chain ID -out PATH
//	backup-seal  -code-file F -height H -in DIR -light-blocks DIR -out PATH
//	backup-check -paper F -in F
//	backup-open  -paper F -in F -out DIR
//
// Metric history copies (node/docs/architecture/monitoring-v1.md) encrypt
// one host's finished day of history under the chain's history code:
//
//	history-code  -chain ID -out PATH
//	history-seal  -code-file F -host LABEL -day YYYY-MM-DD -in F -out PATH
//	history-check -paper F -in F
//	history-open  -paper F -in F -out PATH
//
// Outputs are never overwritten. Everything it writes except private keys,
// a kit's paper line, unsealed files and backup and history codes is a public record. It cannot start a chain,
// consume a sequence or approve a launch: the node verifies every signature
// again through its pinned helper.
package main

import (
	"crypto/rand"
	"crypto/sha256"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	root "dytallix.local/consensus/root-authorization"
	"github.com/cloudflare/circl/sign/slhdsa"
)

const (
	maxRecordBytes    = 1 << 20
	maxGenesisBytes   = 64 << 20
	privateKeyFileMod = 0o600
	publicFileMode    = 0o644
)

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "dytallix-root-sign:", err)
		os.Exit(1)
	}
}

func run(args []string, out io.Writer) error {
	if len(args) == 0 {
		return errors.New("a command is required: keygen, policy, digest, sign, combine or verify")
	}
	flags := flag.NewFlagSet("dytallix-root-sign "+args[0], flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	switch args[0] {
	case "keygen":
		private := flags.String("private-key-out", "", "new private key file (mode 0600)")
		public := flags.String("public-key-out", "", "new public key record")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return keygen(*private, *public, out)
	case "policy":
		chain := flags.String("chain-id", "", "chain the signers sign for")
		output := flags.String("out", "", "new signer policy")
		if err := parse(flags, args[1:], root.GenesisSignerCount); err != nil {
			return err
		}
		return policy(*chain, flags.Args(), *output, out)
	case "digest":
		app := flags.String("native-genesis", "", "native genesis file")
		config := flags.String("config", "", "application configuration file")
		engine := flags.String("engine-genesis", "", "engine genesis file")
		release := flags.String("release-manifest", "", "release manifest file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return digest(*app, *config, *engine, *release, out)
	case "sign":
		policyPath := flags.String("policy", "", "signer policy")
		key := flags.String("private-key", "", "this signer's private key file")
		bundle := flags.String("bundle-sha512", "", "root bundle SHA-512 (hex)")
		output := flags.String("out", "", "new signature file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return sign(*policyPath, *key, *bundle, *output, out)
	case "combine":
		policyPath := flags.String("policy", "", "signer policy")
		output := flags.String("out", "", "new combined signatures file")
		if err := parse(flags, args[1:], -1); err != nil {
			return err
		}
		return combine(*policyPath, flags.Args(), *output, out)
	case "verify":
		policyPath := flags.String("policy", "", "signer policy")
		signatures := flags.String("signatures", "", "combined signatures file")
		bundle := flags.String("bundle-sha512", "", "root bundle SHA-512 (hex)")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return verify(*policyPath, *signatures, *bundle, out)
	case "kit":
		number := flags.Int("number", 0, "kit number, 1 to 5")
		private := flags.String("private-out", "", "the kit drive's directory")
		public := flags.String("public-out", "", "the public records directory")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return kit(*number, *private, *public, out)
	case "kit-check":
		paper := flags.String("paper", "", "the kit's paper line, typed into a file (- for standard input)")
		public := flags.String("public", "", "the public records directory")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return kitCheck(*paper, *public, "", out)
	case "kit-restore":
		paper := flags.String("paper", "", "the kit's paper line, typed into a file (- for standard input)")
		public := flags.String("public", "", "the public records directory")
		private := flags.String("private-out", "", "a new kit drive's directory")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return kitCheck(*paper, *public, *private, out)
	case "prove":
		key := flags.String("private-key", "", "the private key file")
		public := flags.String("public-key", "", "its public key record")
		chain := flags.String("chain-id", "", "chain the key serves")
		controller := flags.String("controller", "", "the controller ID in the intake")
		purpose := flags.String("purpose", "", "genesis, upgrade, freeze or resume")
		epoch := flags.String("epoch", "", "authority epoch, or none for genesis")
		session := flags.String("session", "", "ceremony session ID")
		output := flags.String("out", "", "new proof record")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return prove(*key, *public, *chain, *controller, *purpose, *epoch, *session, *output, out)
	case "verify-proof":
		proof := flags.String("proof", "", "proof record")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return verifyProof(*proof, out)
	case "show-control":
		request := flags.String("request", "", "control request")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return showControl(*request, out)
	case "sign-control":
		request := flags.String("request", "", "control request")
		key := flags.String("private-key", "", "this key's private key file")
		public := flags.String("public-key", "", "its public key record")
		operation := flags.String("operation", "", "the operation you expect, as show-control prints it")
		sequence := flags.String("sequence", "", "the sequence you expect, as show-control prints it")
		output := flags.String("out", "", "new signature file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return signControl(*request, *key, *public, *operation, *sequence, *output, out)
	case "verify-control":
		request := flags.String("request", "", "control request")
		signature := flags.String("signature", "", "signature file")
		public := flags.String("public-key", "", "the signer's public key record")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return verifyControl(*request, *signature, *public, out)
	case "seal":
		label := flags.String("label", "", "the host's pin plan label")
		home := flags.String("home", "", "the staging node home holding the files")
		output := flags.String("out", "", "new sealed record")
		if err := parse(flags, args[1:], -1); err != nil {
			return err
		}
		return seal(*label, *home, *output, flags.Args(), out)
	case "seal-check":
		paper := flags.String("paper", "", "the seal code's paper line, or - to type it")
		sealed := flags.String("sealed", "", "sealed record")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return sealCheck(*paper, *sealed, out)
	case "unseal":
		paper := flags.String("paper", "", "the seal code's paper line, or - to type it")
		sealed := flags.String("sealed", "", "sealed record")
		label := flags.String("label", "", "this host's label")
		output := flags.String("out", "", "the node home")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return unseal(*paper, *sealed, *label, *output, out)
	case "backup-code":
		chain := flags.String("chain", "", "the chain the code is for")
		output := flags.String("out", "", "new backup code file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return backupCode(*chain, *output, out)
	case "backup-seal":
		code := flags.String("code-file", "", "the chain's backup code file")
		height := flags.String("height", "", "the snapshot's height")
		input := flags.String("in", "", "the snapshot directory")
		lightBlocks := flags.String("light-blocks", "", "the snapshot's light blocks, heights H to H+2")
		output := flags.String("out", "", "new copy file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return backupSeal(*code, *height, *input, *lightBlocks, *output, out)
	case "backup-check":
		paper := flags.String("paper", "", "the backup code's paper line, or - to type it")
		input := flags.String("in", "", "the copy")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return backupCheck(*paper, *input, out)
	case "backup-open":
		paper := flags.String("paper", "", "the backup code's paper line, or - to type it")
		input := flags.String("in", "", "the copy")
		output := flags.String("out", "", "new directory for the snapshot")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return backupOpen(*paper, *input, *output, out)
	case "history-code":
		chain := flags.String("chain", "", "the chain the code is for")
		output := flags.String("out", "", "new history code file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return historyCode(*chain, *output, out)
	case "history-seal":
		code := flags.String("code-file", "", "the chain's history code file")
		host := flags.String("host", "", "the host's pin plan label")
		day := flags.String("day", "", "the history's UTC day, YYYY-MM-DD")
		input := flags.String("in", "", "the day's history file")
		output := flags.String("out", "", "new copy file")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return historySeal(*code, *host, *day, *input, *output, out)
	case "history-check":
		paper := flags.String("paper", "", "the history code's paper line, or - to type it")
		input := flags.String("in", "", "the copy")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return historyCheck(*paper, *input, out)
	case "history-open":
		paper := flags.String("paper", "", "the history code's paper line, or - to type it")
		input := flags.String("in", "", "the copy")
		output := flags.String("out", "", "new file for the day's history")
		if err := parse(flags, args[1:], 0); err != nil {
			return err
		}
		return historyOpen(*paper, *input, *output, out)
	}
	return fmt.Errorf("unknown command %q", args[0])
}

// parse requires every flag to be set and the given number of positional
// arguments (-1: at least one).
func parse(flags *flag.FlagSet, args []string, positional int) error {
	if err := flags.Parse(args); err != nil {
		return errors.New("invalid arguments")
	}
	missing := false
	flags.VisitAll(func(f *flag.Flag) { missing = missing || f.Value.String() == "" })
	if missing || (positional >= 0 && flags.NArg() != positional) || (positional < 0 && flags.NArg() == 0) {
		return errors.New("every option is required, with the expected input files")
	}
	return nil
}

func keygen(privatePath, publicPath string, out io.Writer) error {
	public, private, err := slhdsa.GenerateKey(rand.Reader, slhdsa.SHAKE_256s)
	if err != nil {
		return err
	}
	privateBytes, err := private.MarshalBinary()
	if err != nil {
		return err
	}
	defer clear(privateBytes)
	publicBytes, err := public.MarshalBinary()
	if err != nil {
		return err
	}
	if err := root.CheckKeyPair(privateBytes, publicBytes); err != nil {
		return err
	}
	record, err := json.Marshal(root.AuthorityKey{KeyID: root.KeyID(publicBytes), PublicKeyHex: hex.EncodeToString(publicBytes)})
	if err != nil {
		return err
	}
	if err := create(privatePath, privateBytes, privateKeyFileMod); err != nil {
		return err
	}
	if err := create(publicPath, record, publicFileMode); err != nil {
		return err
	}
	fmt.Fprintf(out, "key_id %s\n", root.KeyID(publicBytes))
	return nil
}

func policy(chain string, keyPaths []string, output string, out io.Writer) error {
	result := root.GenesisPolicy{Schema: root.GenesisRecordSchema, ChainID: chain, Authority: root.Authority{Threshold: root.GenesisThreshold}}
	for _, path := range keyPaths {
		var key root.AuthorityKey
		if err := readRecord(path, &key); err != nil {
			return fmt.Errorf("%s: %w", path, err)
		}
		result.Authority.Keys = append(result.Authority.Keys, key)
	}
	sortKeys(result.Authority.Keys)
	if err := result.Validate(); err != nil {
		return err
	}
	return writeRecord(output, result, out)
}

func digest(appPath, configPath, enginePath, releasePath string, out io.Writer) error {
	files := make([][]byte, 4)
	for i, path := range []string{appPath, configPath, enginePath, releasePath} {
		raw, err := readBounded(path, maxGenesisBytes)
		if err != nil {
			return fmt.Errorf("%s: %w", path, err)
		}
		if len(raw) == 0 {
			return fmt.Errorf("%s: empty input", path)
		}
		files[i] = raw
	}
	engine, release := sha512.Sum512(files[2]), sha512.Sum512(files[3])
	bundle := sha512.Sum512(root.GenesisBundle(files[0], files[1], engine, release))
	app, config := sha256.Sum256(files[0]), sha256.Sum256(files[1])
	encoded, err := json.MarshalIndent(map[string]string{
		"bundle_sha512":           hex.EncodeToString(bundle[:]),
		"native_genesis_sha256":   hex.EncodeToString(app[:]),
		"config_sha256":           hex.EncodeToString(config[:]),
		"engine_genesis_sha512":   hex.EncodeToString(engine[:]),
		"release_manifest_sha512": hex.EncodeToString(release[:]),
	}, "", "  ")
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "%s\n", encoded)
	return err
}

func sign(policyPath, keyPath, bundleHex, output string, out io.Writer) error {
	var signers root.GenesisPolicy
	if err := readRecord(policyPath, &signers); err != nil {
		return fmt.Errorf("%s: %w", policyPath, err)
	}
	bundle, err := bundleDigest(bundleHex)
	if err != nil {
		return err
	}
	private, err := readPrivateKey(keyPath)
	if err != nil {
		return fmt.Errorf("%s: %w", keyPath, err)
	}
	defer clear(private)
	signed, err := root.SignGenesis(signers, private, bundle)
	if err != nil {
		return err
	}
	fmt.Fprintf(out, "signed root genesis: chain %s, bundle %s, key %s\n", signed.ChainID, signed.BundleSHA512, signed.Signatures[0].KeyID)
	return writeRecord(output, signed, out)
}

func combine(policyPath string, parts []string, output string, out io.Writer) error {
	var signers root.GenesisPolicy
	if err := readRecord(policyPath, &signers); err != nil {
		return fmt.Errorf("%s: %w", policyPath, err)
	}
	var files []root.GenesisSignatures
	for _, path := range parts {
		var part root.GenesisSignatures
		if err := readRecord(path, &part); err != nil {
			return fmt.Errorf("%s: %w", path, err)
		}
		files = append(files, part)
	}
	combined, err := root.CombineGenesis(signers, files)
	if err != nil {
		return err
	}
	fmt.Fprintf(out, "%d of %d genesis signatures verified\n", len(combined.Signatures), root.GenesisSignerCount)
	return writeRecord(output, combined, out)
}

func verify(policyPath, signaturesPath, bundleHex string, out io.Writer) error {
	var signers root.GenesisPolicy
	if err := readRecord(policyPath, &signers); err != nil {
		return fmt.Errorf("%s: %w", policyPath, err)
	}
	var signatures root.GenesisSignatures
	if err := readRecord(signaturesPath, &signatures); err != nil {
		return fmt.Errorf("%s: %w", signaturesPath, err)
	}
	bundle, err := bundleDigest(bundleHex)
	if err != nil {
		return err
	}
	if err := root.VerifyGenesis(signers, signatures, bundle); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "%d of %d genesis signatures verified\n", len(signatures.Signatures), root.GenesisSignerCount)
	return err
}

func sortKeys(keys []root.AuthorityKey) {
	for i := 1; i < len(keys); i++ {
		for j := i; j > 0 && keys[j].KeyID < keys[j-1].KeyID; j-- {
			keys[j], keys[j-1] = keys[j-1], keys[j]
		}
	}
}

func bundleDigest(value string) ([sha512.Size]byte, error) {
	raw, err := hex.DecodeString(value)
	if err != nil || len(raw) != sha512.Size || hex.EncodeToString(raw) != value {
		return [sha512.Size]byte{}, errors.New("the bundle SHA-512 must be 128 lowercase hex digits")
	}
	return [sha512.Size]byte(raw), nil
}

func readBounded(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > limit {
		return nil, errors.New("not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(raw)) > limit {
		return nil, errors.New("file grew beyond its bound")
	}
	return raw, nil
}

func readRecord(path string, destination any) error {
	raw, err := readBounded(path, maxRecordBytes)
	if err != nil {
		return err
	}
	return root.DecodeCanonical(raw, destination)
}

func readPrivateKey(path string) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 {
		return nil, errors.New("the private key must be a regular file readable only by its owner")
	}
	raw, err := readBounded(path, root.PrivateKeySize)
	if err != nil {
		return nil, err
	}
	if len(raw) != root.PrivateKeySize {
		clear(raw)
		return nil, root.ErrKey
	}
	return raw, nil
}

func writeRecord(path string, value any, out io.Writer) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if err := create(path, raw, publicFileMode); err != nil {
		return err
	}
	sum := sha256.Sum256(raw)
	_, err = fmt.Fprintf(out, "wrote %s sha256 %s\n", path, hex.EncodeToString(sum[:]))
	return err
}

// create writes a new file and never replaces an existing one.
func create(path string, raw []byte, mode os.FileMode) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	if _, err := file.Write(raw); err != nil {
		file.Close()
		return err
	}
	if err := file.Sync(); err != nil {
		file.Close()
		return err
	}
	return file.Close()
}
