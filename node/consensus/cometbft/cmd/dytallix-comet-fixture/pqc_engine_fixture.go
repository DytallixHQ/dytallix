package main

import (
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
)

const pqcLoopbackTransport = "dytallix-pqc-loopback-v1"
const pqcSeedLoopbackTransport = "dytallix-pqc-loopback-seed-v1"
const pqcPrivateSeedTransport = "dytallix-pqc-private-seed-v1"

// The struct order is the transport file's canonical JSON field order.
// Public-key pins are full ML-DSA-65 keys; peer IDs alone do not authenticate them.
type pqcFixturePeer struct {
	ID              string `json:"id"`
	PublicKeyBase64 string `json:"public_key_base64"`
	Address         string `json:"address"`
}

type pqcFixtureTransport struct {
	Version              uint16           `json:"version"`
	Profile              string           `json:"profile"`
	Network              string           `json:"network"`
	LocalPublicKeyBase64 string           `json:"local_public_key_base64"`
	Peers                []pqcFixturePeer `json:"peers"`
	HandshakeTimeoutMS   uint64           `json:"handshake_timeout_ms"`
}

func fixtureNodeKey(path, profile string) (*p2p.NodeKey, error) {
	switch profile {
	case legacyLoopbackTransport:
		return p2p.LoadOrGenNodeKey(path)
	case pqcLoopbackTransport:
		// Use a fresh transport identity, separate from the consensus signing key.
		// Never call the legacy Ed25519-generating loader in this profile.
		if _, err := os.Lstat(path); err == nil || !os.IsNotExist(err) {
			return nil, errors.New("PQC transport key path must not exist")
		}
		private, err := mldsa65.GenPrivKey()
		if err != nil {
			return nil, err
		}
		nodeKey := &p2p.NodeKey{PrivKey: private}
		if len(nodeKey.PubKey().Bytes()) != 1952 || nodeKey.PubKey().Type() != mldsa65.KeyType {
			return nil, errors.New("PQC transport identity requires ML-DSA-65")
		}
		if err := nodeKey.SaveAs(path); err != nil {
			return nil, err
		}
		return nodeKey, nil
	case pqcSeedLoopbackTransport, pqcPrivateSeedTransport:
		if _, err := os.Lstat(path); err == nil {
			return nil, errors.New("packed peer key must not exist in seed profile")
		} else if !os.IsNotExist(err) {
			return nil, err
		}
		seedPath := filepath.Join(filepath.Dir(path), "pqc_peer_seed.bin")
		seed := make([]byte, mldsa65.SeedSize)
		defer clear(seed)
		if _, err := rand.Read(seed); err != nil {
			return nil, err
		}
		private, err := mldsa65.GenPrivKeyFromSeed(seed)
		if err != nil {
			return nil, err
		}
		file, err := os.OpenFile(seedPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
		if err != nil {
			return nil, err
		}
		if n, err := file.Write(seed); err != nil || n != len(seed) {
			_ = file.Close()
			if err != nil {
				return nil, err
			}
			return nil, io.ErrShortWrite
		}
		if err := file.Close(); err != nil {
			return nil, err
		}
		return &p2p.NodeKey{PrivKey: private}, nil
	default:
		return nil, errors.New("unsupported fixture transport profile")
	}
}

func writePQCTransportFixture(home, network string, local int, publicKeys [][]byte, basePort int, profile string, peerIPs []string) error {
	if profile != pqcLoopbackTransport && profile != pqcSeedLoopbackTransport && profile != pqcPrivateSeedTransport {
		return errors.New("unsupported PQC transport fixture profile")
	}
	if profile == pqcPrivateSeedTransport && len(peerIPs) != len(publicKeys) {
		return errors.New("private PQC fixture requires one IP for each node")
	}
	if len(network) == 0 || len(network) > 64 || local < 0 || local >= len(publicKeys) {
		return errors.New("invalid PQC transport fixture context")
	}
	if len(publicKeys) < 2 || len(publicKeys) > 6 || basePort < 1024 || basePort > 65535-(len(publicKeys)-1)*10-1 {
		return errors.New("invalid PQC transport fixture peer bounds")
	}
	configuration := pqcFixtureTransport{
		Version: 1, Profile: profile, Network: network,
		LocalPublicKeyBase64: base64.StdEncoding.EncodeToString(publicKeys[local]),
		Peers:                make([]pqcFixturePeer, 0, len(publicKeys)-1), HandshakeTimeoutMS: 5000,
	}
	seen := make(map[p2p.ID]bool, len(publicKeys))
	for index, key := range publicKeys {
		if len(key) != 1952 {
			return errors.New("PQC fixture pin requires a full ML-DSA-65 key")
		}
		public, err := mldsa65.NewPubKeyFromBytes(key)
		if err != nil {
			return err
		}
		id := p2p.PubKeyToID(public)
		if seen[id] {
			return errors.New("duplicate PQC transport fixture identity")
		}
		seen[id] = true
		if index == local {
			continue
		}
		address := fmt.Sprintf("127.0.0.1:%d", basePort+index*10)
		if profile == pqcPrivateSeedTransport {
			address = net.JoinHostPort(peerIPs[index], strconv.Itoa(basePort+index*10))
		}
		configuration.Peers = append(configuration.Peers, pqcFixturePeer{
			ID: string(id), PublicKeyBase64: base64.StdEncoding.EncodeToString(key),
			Address: address,
		})
	}
	encoded, err := json.Marshal(configuration)
	if err != nil {
		return err
	}
	path := filepath.Join(home, "config", "pqc_transport.json")
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	if n, err := file.Write(encoded); err != nil || n != len(encoded) {
		_ = file.Close()
		if err != nil {
			return err
		}
		return io.ErrShortWrite
	}
	return file.Close()
}
