// Package enginepqc installs the experimental PQC record transport in the real
// Comet TCP transport. It does not authorize production or clear artifact gates.
package enginepqc

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/types"
	"golang.org/x/sys/unix"
)

const Profile = "dytallix-pqc-loopback-v1"
const SeedProfile = "dytallix-pqc-loopback-seed-v1"
const RemoteSeedProfile = "dytallix-pqc-private-seed-v1"

// ProductionCandidateProfile selects offline policy checks only. Load and the
// engine command still reject this profile until production is qualified.
const ProductionCandidateProfile = "dytallix-pqc-production-candidate-v1"
const CandidateStagingChainPrefix = "e01-candidate-"
const MaxConcurrentHandshakes = 8
const MaxPeers = 64

type PeerPin struct {
	ID              string `json:"id"`
	PublicKeyBase64 string `json:"public_key_base64"`
	Address         string `json:"address"`
}
type TransportConfig struct {
	Version              uint16    `json:"version"`
	Profile              string    `json:"profile"`
	Network              string    `json:"network"`
	LocalPublicKeyBase64 string    `json:"local_public_key_base64"`
	Peers                []PeerPin `json:"peers"`
	HandshakeTimeoutMS   uint32    `json:"handshake_timeout_ms"`
}
type Runtime struct {
	Config          *cfg.Config
	Genesis         *types.GenesisDoc
	NodeKey         *p2p.NodeKey
	Validator       *privval.FilePV
	Transport       TransportConfig
	identity        *pqcp2p.Identity
	pins            [][]byte
	byID            map[p2p.ID][]byte
	addresses       map[p2p.ID]string
	slots           chan struct{}
	configSHA256    [sha256.Size]byte
	genesisSHA256   [sha256.Size]byte
	transportSHA256 [sha256.Size]byte
}

func privateFile(path string, limit int64) ([]byte, error) {
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	file := os.NewFile(uintptr(fd), path)
	defer file.Close()
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return nil, err
	}
	mode := stat.Mode & 0o777
	if stat.Mode&unix.S_IFMT != unix.S_IFREG || (mode != 0o400 && mode != 0o600) || stat.Size <= 0 || stat.Size > limit || stat.Nlink != 1 || stat.Uid != uint32(os.Geteuid()) {
		return nil, fmt.Errorf("expected bounded private regular file: %s", path)
	}
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) != stat.Size {
		return nil, fmt.Errorf("private file changed during read: %s", path)
	}
	return data, nil
}
func loopback(address string) bool {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return false
	}
	n, err := strconv.Atoi(port)
	ip := net.ParseIP(host)
	return err == nil && n > 0 && n <= 65535 && ip != nil && ip.IsLoopback()
}

// ValidateIsolation checks all supported network entry points before NewNode.
// It allows only the fixed local browser origin, with no TLS/classical fallback.
func ValidateIsolation(c *cfg.Config) error {
	return validateIsolationForProfile(c, Profile)
}

// ValidateProfileIsolation checks a staging or candidate profile before
// fixture creation or engine startup. It never authorizes production.
func ValidateProfileIsolation(c *cfg.Config, profile string) error {
	return validateIsolationForProfile(c, profile)
}

func validateIsolationForProfile(c *cfg.Config, profile string) error {
	if c == nil || c.P2P == nil || c.RPC == nil || c.StateSync == nil || c.Instrumentation == nil {
		return errors.New("incomplete engine configuration")
	}
	if c.PrivValidatorListenAddr != "" || c.P2P.LibP2PEnabled() || c.P2P.ExternalAddress != "" || c.P2P.Seeds != "" || c.P2P.PexReactor || c.P2P.SeedMode || c.StateSync.Enable || c.P2P.TestFuzz || c.RPC.Unsafe || c.RPC.GRPCListenAddress != "" || c.RPC.PprofListenAddress != "" || c.RPC.TLSCertFile != "" || c.RPC.TLSKeyFile != "" || c.Instrumentation.Prometheus {
		return errors.New("PQC engine forbids remote signer, libp2p, discovery, state sync, fuzzing, unsafe RPC and extra listeners")
	}
	if c.ABCI != "socket" || c.ProxyApp != "unix://"+filepath.Join(c.RootDir, "abci", "app.sock") || !strings.HasPrefix(c.P2P.ListenAddress, "tcp://") || !strings.HasPrefix(c.RPC.ListenAddress, "tcp://") || !loopback(strings.TrimPrefix(c.RPC.ListenAddress, "tcp://")) {
		return errors.New("PQC engine requires explicit TCP P2P, loopback RPC and local Unix ABCI")
	}
	p2pAddress := strings.TrimPrefix(c.P2P.ListenAddress, "tcp://")
	if profile == ProductionCandidateProfile {
		if !explicitPeerEndpoint(p2pAddress) || c.P2P.AllowDuplicateIP || !c.P2P.AddrBookStrict || len(c.RPC.CORSAllowedOrigins) != 0 {
			return errors.New("production candidate requires one explicit P2P endpoint, strict peer admission and no browser origin")
		}
	} else if profile == RemoteSeedProfile {
		if !privateEndpoint(p2pAddress) || c.P2P.AllowDuplicateIP || !c.P2P.AddrBookStrict {
			return errors.New("private PQC staging profile requires one explicit private P2P endpoint per host")
		}
	} else if profile == Profile || profile == SeedProfile {
		if !loopback(p2pAddress) {
			return errors.New("PQC loopback profile requires loopback P2P")
		}
	} else {
		return errors.New("unsupported PQC transport profile")
	}
	if len(c.RPC.CORSAllowedOrigins) > 1 || (len(c.RPC.CORSAllowedOrigins) == 1 && c.RPC.CORSAllowedOrigins[0] != "http://127.0.0.1:4173") {
		return errors.New("only the explicit local browser origin http://127.0.0.1:4173 is allowed")
	}
	if c.P2P.UnconditionalPeerIDs != "" || c.P2P.PrivatePeerIDs != "" || c.P2P.MaxNumInboundPeers < 1 || c.P2P.MaxNumInboundPeers > MaxPeers || c.P2P.MaxNumOutboundPeers < 1 || c.P2P.MaxNumOutboundPeers > MaxPeers {
		return errors.New("experimental PQC engine requires bounded ordinary peer admission")
	}
	if c.GenesisFile() != filepath.Join(c.RootDir, "config", "genesis.json") || c.NodeKeyFile() != filepath.Join(c.RootDir, "config", "node_key.json") || c.PrivValidatorKeyFile() != filepath.Join(c.RootDir, "config", "priv_validator_key.json") || c.PrivValidatorStateFile() != filepath.Join(c.RootDir, "data", "priv_validator_state.json") || c.DBDir() != filepath.Join(c.RootDir, "data") {
		return errors.New("experimental PQC engine requires its existing local fixture paths")
	}
	return c.ValidateBasic()
}

func decodePin(encoded string) ([]byte, error) {
	raw, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(raw) != mldsa65.PubKeySize || base64.StdEncoding.EncodeToString(raw) != encoded {
		return nil, errors.New("peer pin must be canonical full ML-DSA-65 public bytes")
	}
	if _, err = mldsa65.NewPubKeyFromBytes(raw); err != nil {
		return nil, err
	}
	return raw, nil
}

func validatePins(tc TransportConfig, c *cfg.Config, key *p2p.NodeKey, chain string) ([][]byte, map[p2p.ID][]byte, map[p2p.ID]string, error) {
	return validatePinsForProfile(tc, c, key, chain, Profile)
}

func validatePinsForProfile(tc TransportConfig, c *cfg.Config, key *p2p.NodeKey, chain, profile string) ([][]byte, map[p2p.ID][]byte, map[p2p.ID]string, error) {
	fail := func(message string) ([][]byte, map[p2p.ID][]byte, map[p2p.ID]string, error) {
		return nil, nil, nil, errors.New(message)
	}
	if tc.Version != 1 || tc.Profile != profile || tc.Network != chain || len(tc.Peers) < 1 || len(tc.Peers) > MaxPeers || tc.HandshakeTimeoutMS < 100 || tc.HandshakeTimeoutMS > 10000 {
		return fail("invalid explicit PQC transport profile or bounds")
	}
	local, err := decodePin(tc.LocalPublicKeyBase64)
	if err != nil || !bytes.Equal(local, key.PubKey().Bytes()) {
		return fail("PQC local public pin differs from the loaded peer key")
	}
	pins := make([][]byte, 0, len(tc.Peers))
	byID := make(map[p2p.ID][]byte)
	addresses := make(map[p2p.ID]string)
	seenAddr := make(map[string]bool)
	for _, peer := range tc.Peers {
		pin, err := decodePin(peer.PublicKeyBase64)
		if err != nil {
			return nil, nil, nil, err
		}
		public, err := mldsa65.NewPubKeyFromBytes(pin)
		if err != nil {
			return nil, nil, nil, err
		}
		id := p2p.PubKeyToID(public)
		addressAllowed := loopback(peer.Address)
		if profile == RemoteSeedProfile || profile == ProductionCandidateProfile {
			if profile == ProductionCandidateProfile {
				addressAllowed = explicitPeerEndpoint(peer.Address)
			} else {
				addressAllowed = privateEndpoint(peer.Address)
			}
			addressAllowed = addressAllowed && !endpointIP(peer.Address).Equal(endpointIP(strings.TrimPrefix(c.P2P.ListenAddress, "tcp://")))
			for _, existing := range addresses {
				if endpointIP(existing).Equal(endpointIP(peer.Address)) {
					addressAllowed = false
				}
			}
		}
		if string(id) != peer.ID || id == key.ID() || bytes.Equal(pin, local) || byID[id] != nil || !addressAllowed || seenAddr[peer.Address] {
			return fail("peer allowlist contains an invalid ID, self key, duplicate or nonlocal address")
		}
		byID[id] = bytes.Clone(pin)
		addresses[id] = peer.Address
		seenAddr[peer.Address] = true
		pins = append(pins, bytes.Clone(pin))
	}
	configured := strings.Split(c.P2P.PersistentPeers, ",")
	if len(configured) != len(tc.Peers) {
		return fail("persistent peer set differs from full-key allowlist")
	}
	seen := make(map[p2p.ID]bool)
	for _, text := range configured {
		idText, address, ok := strings.Cut(text, "@")
		id := p2p.ID(idText)
		if !ok || byID[id] == nil || addresses[id] != address || seen[id] {
			return fail("persistent peer address is not bound to its full-key pin")
		}
		seen[id] = true
	}
	return pins, byID, addresses, nil
}

// Load opens only staging profiles. It never creates or replaces an identity
// or signer state. No database or network service starts until guards pass.
func Load(home, profile string) (*Runtime, error) {
	return load(home, profile, false)
}

// ValidateProductionCandidateFiles checks existing candidate files without
// returning the runtime or starting a network service. It does not authorize
// the engine command's production mode.
func ValidateProductionCandidateFiles(home string) error {
	_, err := load(home, ProductionCandidateProfile, true)
	return err
}

// LoadCandidateForStaging exercises the candidate runtime on a reserved
// staging chain. It cannot load a production chain identity.
func LoadCandidateForStaging(home string) (*Runtime, error) {
	runtime, err := load(home, ProductionCandidateProfile, true)
	if err != nil {
		return nil, err
	}
	if !candidateStagingChain(runtime.Genesis.ChainID) {
		return nil, errors.New("candidate startup requires the reserved E01 staging chain prefix")
	}
	return runtime, nil
}

func candidateStagingChain(chainID string) bool {
	lower := strings.ToLower(chainID)
	return strings.HasPrefix(chainID, CandidateStagingChainPrefix) && len(chainID) > len(CandidateStagingChainPrefix) && !strings.Contains(lower, "mainnet") && !strings.Contains(lower, "production")
}

func load(home, profile string, candidate bool) (*Runtime, error) {
	allowed := (profile == Profile || profile == SeedProfile || profile == RemoteSeedProfile) && !candidate
	if candidate {
		allowed = profile == ProductionCandidateProfile
	}
	if !allowed || !filepath.IsAbs(home) || filepath.Clean(home) != home {
		return nil, errors.New("supported PQC profile and clean absolute home are required")
	}
	if (profile == RemoteSeedProfile || candidate) && (BuildProfile != "dytallix_pqc_only" || RPCBuildProfile != "dytallix-pqc-unix-v1") {
		return nil, errors.New("seed-backed PQC profile requires the selected PQC-only IPC build")
	}
	for _, path := range []string{home, filepath.Join(home, "config"), filepath.Join(home, "data"), filepath.Join(home, "abci")} {
		stat, err := os.Lstat(path)
		if err != nil {
			return nil, err
		}
		if !stat.IsDir() || stat.Mode()&os.ModeSymlink != 0 || stat.Mode().Perm()&0077 != 0 {
			return nil, errors.New("private existing fixture directories required")
		}
	}
	configRaw, err := privateFile(filepath.Join(home, "config", "config.toml"), 1<<20)
	if err != nil {
		return nil, err
	}
	c, err := decodeConfig(configRaw)
	if err != nil {
		return nil, err
	}
	c.SetRoot(home)
	if err = validateIsolationForProfile(c, profile); err != nil {
		return nil, err
	}
	genesisRaw, err := privateFile(c.GenesisFile(), 2<<20)
	if err != nil {
		return nil, err
	}
	genesis, err := types.GenesisDocFromJSON(genesisRaw)
	if err != nil {
		return nil, err
	}
	lower := strings.ToLower(genesis.ChainID)
	if genesis.ChainID == "" || len(genesis.ChainID) > 64 || (!candidate && (strings.Contains(lower, "mainnet") || strings.Contains(lower, "production"))) || genesis.InitialHeight != 1 || len(genesis.Validators) < 1 || len(genesis.Validators) > MaxPeers {
		return nil, errors.New("bounded genesis with the selected profile is required")
	}
	if len(genesis.ConsensusParams.Validator.PubKeyTypes) != 1 || genesis.ConsensusParams.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		return nil, errors.New("genesis must select only ML-DSA-65 validator keys")
	}
	for _, validator := range genesis.Validators {
		if validator.PubKey == nil || validator.PubKey.Type() != mldsa65.KeyType || len(validator.PubKey.Bytes()) != mldsa65.PubKeySize || validator.Power <= 0 {
			return nil, errors.New("genesis contains an unsupported validator key or power")
		}
	}
	raw, err := privateFile(filepath.Join(home, "config", "pqc_transport.json"), 256<<10)
	if err != nil {
		return nil, err
	}
	var tc TransportConfig
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&tc); err != nil {
		return nil, err
	}
	if err = decoder.Decode(new(any)); err != io.EOF {
		return nil, errors.New("trailing transport configuration data")
	}
	canonical, _ := json.Marshal(tc)
	if !bytes.Equal(canonical, bytes.TrimSpace(raw)) {
		return nil, errors.New("transport configuration must use canonical compact JSON")
	}
	if tc.Profile != profile {
		return nil, errors.New("transport configuration profile differs from requested profile")
	}
	var key *p2p.NodeKey
	var identity *pqcp2p.Identity
	if profile == SeedProfile || profile == RemoteSeedProfile || candidate {
		local, err := decodePin(tc.LocalPublicKeyBase64)
		if err != nil {
			return nil, err
		}
		key, identity, err = seedNodeKey(home, local)
		if err != nil {
			return nil, err
		}
	} else {
		keyRaw, err := privateFile(c.NodeKeyFile(), 16384)
		if err != nil {
			return nil, err
		}
		key = new(p2p.NodeKey)
		if err = cmtjson.Unmarshal(keyRaw, key); err != nil {
			return nil, err
		}
		if key.PrivKey == nil || key.PrivKey.Type() != mldsa65.KeyType {
			return nil, errors.New("classical and non-ML-DSA-65 peer identities are forbidden")
		}
		packed := key.PrivKey.Bytes()
		identity, err = pqcp2p.ImportIdentity(packed)
		clear(packed)
		if err != nil {
			return nil, err
		}
		if !bytes.Equal(identity.PublicKey(), key.PubKey().Bytes()) {
			return nil, errors.New("peer public identity mismatch")
		}
	}
	pins, byID, addresses, err := validatePinsForProfile(tc, c, key, genesis.ChainID, profile)
	if err != nil {
		return nil, err
	}
	if candidate {
		if err := ValidateProductionTransportCandidate(c, tc, key, genesis.ChainID); err != nil {
			return nil, err
		}
	}
	validatorRaw, err := privateFile(c.PrivValidatorKeyFile(), 16384)
	if err != nil {
		return nil, err
	}
	var validatorKey privval.FilePVKey
	if err = cmtjson.Unmarshal(validatorRaw, &validatorKey); err != nil {
		return nil, err
	}
	if validatorKey.PrivKey == nil || validatorKey.PubKey == nil || validatorKey.PrivKey.Type() != mldsa65.KeyType || validatorKey.PubKey.Type() != mldsa65.KeyType || !bytes.Equal(validatorKey.PrivKey.PubKey().Bytes(), validatorKey.PubKey.Bytes()) || !bytes.Equal(validatorKey.Address, validatorKey.PubKey.Address()) || bytes.Equal(validatorKey.PubKey.Bytes(), key.PubKey().Bytes()) {
		return nil, errors.New("validator key must be a separate, consistent ML-DSA-65 identity")
	}
	stateRaw, err := privateFile(c.PrivValidatorStateFile(), 16384)
	if err != nil {
		return nil, err
	}
	validator, err := privval.LoadFilePVCheckedBytes(validatorRaw, stateRaw, c.PrivValidatorKeyFile(), c.PrivValidatorStateFile())
	if err != nil {
		return nil, err
	}
	return &Runtime{
		Config: c, Genesis: genesis, NodeKey: key, Validator: validator,
		Transport: tc, identity: identity, pins: pins, byID: byID,
		addresses: addresses, slots: make(chan struct{}, MaxConcurrentHandshakes),
		configSHA256:    sha256.Sum256(configRaw),
		genesisSHA256:   sha256.Sum256(genesisRaw),
		transportSHA256: sha256.Sum256(raw),
	}, nil
}

type authenticatedConn struct {
	*pqcp2p.Conn
	public crypto.PubKey
}

func (c *authenticatedConn) RemotePubKey() crypto.PubKey { return c.public }

// Upgrade runs on both real transport paths before NodeInfo. A bounded slot is
// acquired before PQC work, including inbound responder key generation.
func (r *Runtime) Upgrade(logger log.Logger) p2p.AuthenticatedConnUpgrade {
	return func(raw net.Conn, dialed *p2p.NetAddress, _ time.Duration) (p2p.AuthenticatedConn, error) {
		select {
		case r.slots <- struct{}{}:
			defer func() { <-r.slots }()
		default:
			return nil, errors.New("PQC handshake capacity reached")
		}
		var remote []byte
		direction := "inbound"
		if dialed != nil {
			direction = "outbound"
			remote = r.byID[dialed.ID]
			if remote == nil || r.addresses[dialed.ID] != net.JoinHostPort(dialed.IP.String(), strconv.Itoa(int(dialed.Port))) {
				return nil, errors.New("outbound address has no exact full-key pin")
			}
		}
		if r.Transport.Profile == RemoteSeedProfile || r.Transport.Profile == ProductionCandidateProfile {
			listenerAddress := strings.TrimPrefix(r.Config.P2P.ListenAddress, "tcp://")
			endpointAllowed := privateEndpoint
			if r.Transport.Profile == ProductionCandidateProfile {
				endpointAllowed = explicitPeerEndpoint
			}
			if !endpointAllowed(raw.LocalAddr().String()) || !endpointAllowed(raw.RemoteAddr().String()) || !endpointIP(raw.LocalAddr().String()).Equal(endpointIP(listenerAddress)) || (dialed == nil && raw.LocalAddr().String() != listenerAddress) {
				return nil, errors.New("PQC pinned connection has an unapproved endpoint")
			}
			remoteIP := endpointIP(raw.RemoteAddr().String())
			if dialed != nil {
				if raw.RemoteAddr().String() != r.addresses[dialed.ID] {
					return nil, errors.New("outbound live endpoint differs from peer pin")
				}
			} else {
				allowed := false
				for _, address := range r.addresses {
					if remoteIP.Equal(endpointIP(address)) {
						allowed = true
						break
					}
				}
				if !allowed {
					return nil, errors.New("inbound source IP has no peer pin")
				}
			}
		} else if !loopback(raw.RemoteAddr().String()) || !loopback(raw.LocalAddr().String()) {
			return nil, errors.New("PQC fixture connection is not loopback")
		}
		connection, err := pqcp2p.Upgrade(raw, r.Transport.Network, r.identity, r.pins, remote, time.Duration(r.Transport.HandshakeTimeoutMS)*time.Millisecond)
		if err != nil {
			return nil, err
		}
		public, err := mldsa65.NewPubKeyFromBytes(connection.PeerPublicKey())
		if err != nil {
			connection.Close()
			return nil, err
		}
		if pin := r.byID[p2p.PubKeyToID(public)]; pin == nil || !bytes.Equal(pin, public.Bytes()) {
			connection.Close()
			return nil, errors.New("authenticated peer differs from full-key allowlist")
		}
		if (r.Transport.Profile == RemoteSeedProfile || r.Transport.Profile == ProductionCandidateProfile) && !endpointIP(raw.RemoteAddr().String()).Equal(endpointIP(r.addresses[p2p.PubKeyToID(public)])) {
			connection.Close()
			return nil, errors.New("authenticated peer key differs from pinned source IP")
		}
		logger.Info("PQC transport authenticated", "direction", direction, "peer_id", p2p.PubKeyToID(public), "suite", pqcp2p.Suite)
		return &authenticatedConn{connection, public}, nil
	}
}

func (r *Runtime) PublicSummary() map[string]any {
	ids := make([]string, 0, len(r.byID))
	for id := range r.byID {
		ids = append(ids, string(id))
	}
	sort.Strings(ids)
	return map[string]any{"build_profile": BuildProfile, "rpc_build_profile": RPCBuildProfile, "classical_executable_exclusion_complete": false, "profile": r.Transport.Profile, "suite": pqcp2p.Suite, "network": r.Transport.Network, "node_id": r.NodeKey.ID(), "peer_ids": ids, "full_key_pin_bytes": mldsa65.PubKeySize, "max_concurrent_handshakes": MaxConcurrentHandshakes, "production_qualified": false, "legacy_fallback": false}
}
