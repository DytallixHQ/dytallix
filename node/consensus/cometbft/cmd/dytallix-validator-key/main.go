// Validator consensus keys for operators (E04 gap 17, T-a; P01 28 September
// 2026). Under host setup v1 generate runs offline on the ceremony machine,
// and the key reaches its host only sealed in the host's bundle.
//
//	dytallix-validator-key generate --key-file FILE --state-file FILE
//	dytallix-validator-key proof --key-file FILE --genesis ENGINE_GENESIS \
//	    --operation register|rotate --validator ID --owner OWNER \
//	    --nonce N --expiry-height H [--amount-udgt A]
//
// generate writes a new ML-DSA-65 key and a fresh signing state, owner-only,
// and never overwrites a file. proof signs the possession proof that
// ValidatorRegister or ValidatorRotateKey carries. It signs only a proof it
// builds itself, for the chain in the engine genesis and naming the key it
// holds, and prints the action for `dytallix ordinary prepare --actions`.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"strconv"
	"unicode"
	"unicode/utf8"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/types"
)

// proofDomain and the tuple below are the node's proof_sign_bytes
// (runtime/validator_lifecycle.rs); the golden vector in main_test.go is
// asserted on both sides.
const proofDomain = "dytallix-validator-key-proof-v1\x00"

// outputVersion versions what this command prints (interfaces v1).
const outputVersion = 1

const maxKeyFileBytes = 16 * 1024

// maxGenesisBytes is the engine's genesis bound (production activation v1, A6).
const maxGenesisBytes = 9 * 1024 * 1024

func usage() error {
	return errors.New("usage: dytallix-validator-key generate --key-file FILE --state-file FILE\n" +
		"       dytallix-validator-key proof --key-file FILE --genesis ENGINE_GENESIS --operation register|rotate " +
		"--validator ID --owner OWNER --nonce N --expiry-height H [--amount-udgt A]")
}

// createNew writes a new owner-only file and never replaces one.
func createNew(path string, data []byte) error {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return fmt.Errorf("%s must be a clean absolute path", path)
	}
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return fmt.Errorf("cannot create %s: %w", path, err)
	}
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	return err
}

func generate(keyFile, stateFile string) (map[string]any, error) {
	if keyFile == stateFile {
		return nil, errors.New("key and state files must differ")
	}
	key, err := mldsa65.GenPrivKey()
	if err != nil {
		return nil, err
	}
	pv := privval.NewFilePV(key, keyFile, stateFile)
	keyJSON, err := cmtjson.MarshalIndent(pv.Key, "", "  ")
	if err != nil {
		return nil, err
	}
	stateJSON, err := cmtjson.MarshalIndent(pv.LastSignState, "", "  ")
	if err != nil {
		return nil, err
	}
	// The state first: a key without its state file is never left behind.
	if err = createNew(stateFile, stateJSON); err != nil {
		return nil, err
	}
	if err = createNew(keyFile, keyJSON); err != nil {
		return nil, err
	}
	return map[string]any{"version": outputVersion, "address": pv.Key.Address.String(),
		"public_key_base64": base64.StdEncoding.EncodeToString(pv.Key.PubKey.Bytes())}, nil
}

// readPrivate reads a bounded regular file that only its owner can read.
func readPrivate(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 || info.Size() > limit {
		return nil, fmt.Errorf("%s must be a regular owner-only file of at most %d bytes", path, limit)
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err == nil && int64(len(data)) > limit {
		err = fmt.Errorf("%s grew beyond %d bytes", path, limit)
	}
	return data, err
}

func loadKey(path string) (privval.FilePVKey, error) {
	var key privval.FilePVKey
	raw, err := readPrivate(path, maxKeyFileBytes)
	if err != nil {
		return key, err
	}
	if err = cmtjson.Unmarshal(raw, &key); err != nil {
		return key, fmt.Errorf("invalid validator key file: %w", err)
	}
	if key.PrivKey == nil || key.PubKey == nil || key.PrivKey.Type() != mldsa65.KeyType ||
		key.PubKey.Type() != mldsa65.KeyType || !bytes.Equal(key.PrivKey.PubKey().Bytes(), key.PubKey.Bytes()) ||
		!bytes.Equal(key.Address, key.PubKey.Address()) {
		return key, errors.New("the key file is not a consistent ML-DSA-65 validator key")
	}
	return key, nil
}

// validID is the node's lifecycle identifier rule: non-empty, at most 256
// bytes, no whitespace or control character.
func validID(name, value string) error {
	if value == "" || len(value) > 256 || !utf8.ValidString(value) {
		return fmt.Errorf("invalid %s", name)
	}
	for _, r := range value {
		if unicode.IsSpace(r) || unicode.IsControl(r) {
			return fmt.Errorf("invalid %s", name)
		}
	}
	return nil
}

// proofBytes is the node's proof_sign_bytes: the domain, then the tuple as
// compact JSON (serde's escaping; HTML escaping off).
func proofBytes(chain, operation, validator, owner, key string, nonce, expiry uint64, amount *big.Int) ([]byte, error) {
	for name, value := range map[string]string{"chain": chain, "validator": validator, "owner": owner} {
		if err := validID(name, value); err != nil {
			return nil, err
		}
	}
	if amount.Sign() < 0 || amount.BitLen() > 128 {
		return nil, errors.New("amount must fit an unsigned 128-bit integer")
	}
	if !(operation == "register" && amount.Sign() > 0) && !(operation == "rotate" && amount.Sign() == 0) {
		return nil, errors.New("register needs a positive amount and rotate none")
	}
	var buffer bytes.Buffer
	encoder := json.NewEncoder(&buffer)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode([]any{chain, operation, validator, owner, key, nonce, expiry, amount.String()}); err != nil {
		return nil, err
	}
	return append([]byte(proofDomain), bytes.TrimSuffix(buffer.Bytes(), []byte("\n"))...), nil
}

func toNumbers(data []byte) []int {
	numbers := make([]int, len(data))
	for i, b := range data {
		numbers[i] = int(b)
	}
	return numbers
}

type proofInput struct {
	keyFile, genesis, operation, validator, owner, amount string
	nonce, expiry                                         uint64
}

func proof(in proofInput) (map[string]any, error) {
	key, err := loadKey(in.keyFile)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(in.genesis)
	if err != nil {
		return nil, err
	}
	if info.Size() > maxGenesisBytes {
		return nil, errors.New("engine genesis exceeds its bound")
	}
	genesis, err := types.GenesisDocFromFile(in.genesis)
	if err != nil {
		return nil, fmt.Errorf("invalid engine genesis: %w", err)
	}
	amount, ok := new(big.Int).SetString(in.amount, 10)
	if !ok || amount.String() != in.amount {
		return nil, errors.New("amount must be a canonical decimal")
	}
	public := key.PubKey.Bytes()
	encoded := base64.StdEncoding.EncodeToString(public)
	payload, err := proofBytes(genesis.ChainID, in.operation, in.validator, in.owner, encoded, in.nonce, in.expiry, amount)
	if err != nil {
		return nil, err
	}
	signature, err := key.PrivKey.Sign(payload)
	if err != nil {
		return nil, err
	}
	if !key.PubKey.VerifySignature(payload, signature) {
		return nil, errors.New("the proof does not verify under the key")
	}
	action := map[string]any{"validator_id": in.validator, "consensus_key": toNumbers(public),
		"proof": toNumbers(signature), "proof_expiry_height": strconv.FormatUint(in.expiry, 10)}
	if in.operation == "register" {
		action["type"] = "ValidatorRegister"
		action["amount_udgt"] = amount.String()
	} else {
		action["type"] = "ValidatorRotateKey"
	}
	return map[string]any{"version": outputVersion, "chain_id": genesis.ChainID, "operation": in.operation,
		"address": key.Address.String(), "action": action}, nil
}

func run(args []string) (map[string]any, error) {
	if len(args) == 0 {
		return nil, usage()
	}
	flags := flag.NewFlagSet("dytallix-validator-key "+args[0], flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	keyFile := flags.String("key-file", "", "validator key file (priv_validator_key.json)")
	switch args[0] {
	case "generate":
		stateFile := flags.String("state-file", "", "new signing state file (priv_validator_state.json)")
		if err := flags.Parse(args[1:]); err != nil || flags.NArg() != 0 || *keyFile == "" || *stateFile == "" {
			return nil, usage()
		}
		return generate(*keyFile, *stateFile)
	case "proof":
		var in proofInput
		flags.StringVar(&in.genesis, "genesis", "", "engine genesis (config/genesis.json)")
		flags.StringVar(&in.operation, "operation", "", "register or rotate")
		flags.StringVar(&in.validator, "validator", "", "validator ID")
		flags.StringVar(&in.owner, "owner", "", "operator account that submits the action")
		flags.Uint64Var(&in.nonce, "nonce", 0, "the operator account's spending nonce for the action")
		flags.Uint64Var(&in.expiry, "expiry-height", 0, "last height at which the proof is valid")
		flags.StringVar(&in.amount, "amount-udgt", "0", "self-bond for register")
		if err := flags.Parse(args[1:]); err != nil || flags.NArg() != 0 || *keyFile == "" || in.genesis == "" ||
			in.validator == "" || in.owner == "" || in.expiry == 0 {
			return nil, usage()
		}
		in.keyFile = *keyFile
		return proof(in)
	default:
		return nil, usage()
	}
}

func main() {
	result, err := run(os.Args[1:])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	_ = json.NewEncoder(os.Stdout).Encode(result)
}
