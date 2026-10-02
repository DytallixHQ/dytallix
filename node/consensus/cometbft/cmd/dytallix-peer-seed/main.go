// The production peer identity and host binding (production activation v1,
// A5). It runs on the node's host, so the seed never leaves it.
//
//	dytallix-peer-seed generate --home HOME
//	dytallix-peer-seed public --home HOME
//	dytallix-peer-seed binding --home HOME --role validator|sentry|endpoint
//
// generate writes HOME/config/pqc_peer_seed.bin: fresh bytes from the
// operating system, owner-only, never replacing a file. The production
// transport derives the node's ML-DSA-65 peer key from it and refuses a
// packed node_key.json. generate and public print that public key for the
// node's own transport file and its peers' pins. binding prints the host's
// binding record (A4) for the published pin plan; it loads the home with
// the production transport profile, so only a production build runs it.
package main

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
)

// outputVersion versions what generate and public print (interfaces v1).
const outputVersion = 1

func usage() error {
	return errors.New("usage: dytallix-peer-seed generate --home HOME\n" +
		"       dytallix-peer-seed public --home HOME\n" +
		"       dytallix-peer-seed binding --home HOME --role validator|sentry|endpoint")
}

// configDir is HOME/config, an existing owner-only directory.
func configDir(home string) (string, error) {
	if !filepath.IsAbs(home) || filepath.Clean(home) != home {
		return "", errors.New("--home must be a clean absolute path")
	}
	dir := filepath.Join(home, "config")
	info, err := os.Lstat(dir)
	if err != nil {
		return "", err
	}
	if !info.IsDir() || info.Mode().Perm()&0o077 != 0 {
		return "", errors.New("HOME/config must be an existing owner-only directory")
	}
	return dir, nil
}

// publicRecord is what generate and public print.
func publicRecord(public []byte) (map[string]any, error) {
	key, err := mldsa65.NewPubKeyFromBytes(public)
	if err != nil {
		return nil, err
	}
	sum := sha256.Sum256(public)
	return map[string]any{"version": outputVersion,
		"public_key_base64": base64.StdEncoding.EncodeToString(public),
		"public_key_sha256": hex.EncodeToString(sum[:]),
		"peer_id":           string(p2p.PubKeyToID(key))}, nil
}

func generate(home string) (map[string]any, error) {
	dir, err := configDir(home)
	if err != nil {
		return nil, err
	}
	if _, err := os.Lstat(filepath.Join(dir, "node_key.json")); err == nil {
		return nil, errors.New("the production transport refuses a packed peer key; this home has node_key.json")
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	seed := make([]byte, mldsa65.SeedSize)
	defer clear(seed)
	if _, err := io.ReadFull(rand.Reader, seed); err != nil {
		return nil, err
	}
	private, err := mldsa65.GenPrivKeyFromSeed(seed)
	if err != nil {
		return nil, err
	}
	path := filepath.Join(dir, enginepqc.SeedFileName)
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return nil, fmt.Errorf("cannot create %s: %w", path, err)
	}
	if _, err = file.Write(seed); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return nil, err
	}
	return publicRecord(private.PubKey().Bytes())
}

func public(home string) (map[string]any, error) {
	if _, err := configDir(home); err != nil {
		return nil, err
	}
	key, err := enginepqc.PeerSeedPublicKey(home)
	if err != nil {
		return nil, err
	}
	return publicRecord(key)
}

func binding(home, role string) (enginepqc.ProductionBinding, error) {
	runtime, err := enginepqc.Load(home, enginepqc.ProductionProfile)
	if err != nil {
		return enginepqc.ProductionBinding{}, err
	}
	return enginepqc.NewProductionBinding(runtime, role)
}

func run(args []string) (any, error) {
	if len(args) < 1 {
		return nil, usage()
	}
	flags := flag.NewFlagSet(args[0], flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	home := flags.String("home", "", "the node home")
	role := flags.String("role", "", "validator, sentry or endpoint")
	if err := flags.Parse(args[1:]); err != nil || flags.NArg() != 0 || *home == "" {
		return nil, usage()
	}
	if (args[0] == "binding") != (*role != "") {
		return nil, usage()
	}
	switch args[0] {
	case "generate":
		return generate(*home)
	case "public":
		return public(*home)
	case "binding":
		return binding(*home, *role)
	}
	return nil, usage()
}

func main() {
	result, err := run(os.Args[1:])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	// A binding prints as the canonical compact record the engine reads.
	_ = json.NewEncoder(os.Stdout).Encode(result)
}
