// Host configuration generator (E05). From the public pin plan, the resolved
// host values and the engine genesis, it writes each host's engine files for
// the production transport profile, and the set of bindings the pin plan
// publishes.
//
//	dytallix-host-config --plan PIN_PLAN.json --values HOST_VALUES.json \
//	    --genesis ENGINE_GENESIS --out DIR
//
// DIR (which must not exist) receives hosts/LABEL/config.toml,
// hosts/LABEL/pqc_transport.json and hosts/LABEL/binding.json for every
// host, and PIN_PLAN_BINDINGS.json. Every file is public: the plan holds
// public keys and addresses only, and no private key or seed is read. The
// same inputs always give the same bytes.
//
// It refuses a plan that breaks the approved mesh rules (P01, 30 September
// 2026): one IP per node, validators pinned only by their own sentries,
// endpoints pinned only by sentries, symmetric pins within 64, and no sentry
// or endpoint key in the genesis validator set. A validator's key may be
// outside it: a validator registered after genesis (P01, 3 October 2026).
// Each host's files must pass the engine's own start checks for the
// production profile.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/types"
)

const (
	planSchema     = "dytallix.pin-plan.v1"
	valuesSchema   = "dytallix.host-values.v1"
	bindingsSchema = "dytallix.pin-plan-bindings.v1"
	// The engine serves RPC on its owner-only Unix sockets (rpc profile
	// dytallix-pqc-unix-v1); its configuration names a loopback address it
	// never listens on.
	rpcPlaceholder = "tcp://127.0.0.1:26657"
	maxInputBytes  = 9 << 20
)

var identifier = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`)
var trustHash = regexp.MustCompile(`^[0-9A-Fa-f]{64}$`)

type stateSync struct {
	TrustHeight int64  `json:"trust_height"`
	TrustHash   string `json:"trust_hash"`
}

type host struct {
	Label              string     `json:"label"`
	Operator           string     `json:"operator"`
	Role               string     `json:"role"`
	Home               string     `json:"home"`
	P2P                string     `json:"p2p"`
	Channel            *string    `json:"channel"`
	Status             *string    `json:"status"`
	PeerPublicKey      string     `json:"peer_public_key_base64"`
	ValidatorPublicKey string     `json:"validator_public_key_base64"`
	Pins               []string   `json:"pins"`
	StateSync          *stateSync `json:"state_sync"`
}

type plan struct {
	Schema  string `json:"schema"`
	ChainID string `json:"chain_id"`
	Hosts   []host `json:"hosts"`
}

type values struct {
	Schema    string `json:"schema"`
	Consensus struct {
		TimeoutPropose              string           `json:"timeout_propose"`
		TimeoutProposeDelta         string           `json:"timeout_propose_delta"`
		TimeoutPrevote              string           `json:"timeout_prevote"`
		TimeoutPrevoteDelta         string           `json:"timeout_prevote_delta"`
		TimeoutPrecommit            string           `json:"timeout_precommit"`
		TimeoutPrecommitDelta       string           `json:"timeout_precommit_delta"`
		TimeoutCommit               string           `json:"timeout_commit"`
		SkipTimeoutCommit           bool             `json:"skip_timeout_commit"`
		CreateEmptyBlocks           bool             `json:"create_empty_blocks"`
		CreateEmptyBlocksInterval   string           `json:"create_empty_blocks_interval"`
		PeerGossipSleepDuration     string           `json:"peer_gossip_sleep_duration"`
		PeerQueryMaj23SleepDuration string           `json:"peer_query_maj23_sleep_duration"`
		BlockTimeTolerance          string           `json:"block_time_tolerance"`
		DoubleSignCheckHeight       map[string]int64 `json:"double_sign_check_height"`
	} `json:"consensus"`
	Mempool struct {
		Size           int    `json:"size"`
		MaxTxsBytes    int64  `json:"max_txs_bytes"`
		CacheSize      int    `json:"cache_size"`
		RecheckTimeout string `json:"recheck_timeout"`
		MaxTxBytes     int    `json:"max_tx_bytes"`
	} `json:"mempool"`
	P2P struct {
		MaxNumInboundPeers           int    `json:"max_num_inbound_peers"`
		MaxNumOutboundPeers          int    `json:"max_num_outbound_peers"`
		MaxPacketMsgPayloadSize      int    `json:"max_packet_msg_payload_size"`
		SendRate                     int64  `json:"send_rate"`
		RecvRate                     int64  `json:"recv_rate"`
		FlushThrottleTimeout         string `json:"flush_throttle_timeout"`
		HandshakeTimeout             string `json:"handshake_timeout"`
		DialTimeout                  string `json:"dial_timeout"`
		PersistentPeersMaxDialPeriod string `json:"persistent_peers_max_dial_period"`
	} `json:"p2p"`
	Transport struct {
		HandshakeTimeoutMS uint32 `json:"handshake_timeout_ms"`
	} `json:"transport"`
	StateSync struct {
		DiscoveryTime       string `json:"discovery_time"`
		ChunkRequestTimeout string `json:"chunk_request_timeout"`
		ChunkFetchers       int32  `json:"chunk_fetchers"`
		MaxSnapshotChunks   uint32 `json:"max_snapshot_chunks"`
		TrustPeriod         string `json:"trust_period"`
	} `json:"statesync"`
}

// firewall is the host-policy renderer's firewall input for the host
// (tools/native-execution-policy, routed mode).
type firewall struct {
	TransportSHA256 string   `json:"transport_sha256"`
	P2PListen       string   `json:"p2p_listen"`
	PublicListeners []string `json:"public_listeners"`
}

// hostBinding is one row of the published pin plan.
type hostBinding struct {
	Label         string                      `json:"label"`
	Operator      string                      `json:"operator"`
	Role          string                      `json:"role"`
	Pins          []string                    `json:"pins"`
	Firewall      firewall                    `json:"firewall"`
	Binding       enginepqc.ProductionBinding `json:"binding"`
	BindingSHA256 string                      `json:"binding_sha256"`
}

type bindings struct {
	Schema        string `json:"schema"`
	ChainID       string `json:"chain_id"`
	PlanSHA256    string `json:"plan_sha256"`
	ValuesSHA256  string `json:"values_sha256"`
	GenesisSHA256 string `json:"genesis_sha256"`
	// Genesis validators the plan gives no validator host, such as one that
	// has since left the set; a launch plan should have none.
	GenesisValidatorsWithoutHost int           `json:"genesis_validators_without_host"`
	Hosts                        []hostBinding `json:"hosts"`
}

// output is one generated file.
type output struct {
	path string
	data []byte
}

func digest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// strict decodes JSON with no unknown fields and nothing after it.
func strict(raw []byte, value any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("trailing JSON data")
	}
	return nil
}

// complete decodes strictly and requires every field to be present, so a
// missing value is refused instead of read as zero.
func complete(raw []byte, value any) error {
	if err := strict(raw, value); err != nil {
		return err
	}
	var given, expected any
	canonical, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(raw, &given); err != nil {
		return err
	}
	if err := json.Unmarshal(canonical, &expected); err != nil {
		return err
	}
	return sameFields(given, expected, "")
}

func sameFields(given, expected any, where string) error {
	switch e := expected.(type) {
	case map[string]any:
		g, _ := given.(map[string]any)
		if len(g) != len(e) {
			return fmt.Errorf("%s: missing fields", where)
		}
		for key, value := range e {
			if err := sameFields(g[key], value, where+"."+key); err != nil {
				return err
			}
		}
	case []any:
		g, _ := given.([]any)
		for i := range e {
			if err := sameFields(g[i], e[i], fmt.Sprintf("%s[%d]", where, i)); err != nil {
				return err
			}
		}
	}
	return nil
}

func readBounded(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, maxInputBytes+1))
	if err == nil && len(raw) > maxInputBytes {
		err = fmt.Errorf("%s exceeds %d bytes", path, maxInputBytes)
	}
	return raw, err
}

func duration(name, text string) (time.Duration, error) {
	value, err := time.ParseDuration(text)
	if err != nil || value < 0 {
		return 0, fmt.Errorf("%s: invalid duration %q", name, text)
	}
	return value, nil
}

// publicKey decodes a canonical base64 ML-DSA-65 public key.
func publicKey(name, encoded string) ([]byte, error) {
	raw, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(raw) != mldsa65.PubKeySize || base64.StdEncoding.EncodeToString(raw) != encoded {
		return nil, fmt.Errorf("%s: public key must be canonical base64 of %d bytes", name, mldsa65.PubKeySize)
	}
	if _, err := mldsa65.NewPubKeyFromBytes(raw); err != nil {
		return nil, fmt.Errorf("%s: %w", name, err)
	}
	return raw, nil
}

// endpoint parses a canonical global unicast IP and a port from 1024, as
// the production profile and the host firewall require.
func endpoint(name, text string) (net.IP, int, error) {
	hostPart, portPart, err := net.SplitHostPort(text)
	ip := net.ParseIP(hostPart)
	port, perr := strconv.Atoi(portPart)
	if err != nil || perr != nil || ip == nil || !ip.IsGlobalUnicast() || port < 1024 || port > 65535 || net.JoinHostPort(ip.String(), strconv.Itoa(port)) != text {
		return nil, 0, fmt.Errorf("%s: %q must be a canonical global unicast IP and a port from 1024", name, text)
	}
	return ip, port, nil
}

// checkPlan applies the approved mesh rules to the whole plan.
func checkPlan(p plan, genesis *types.GenesisDoc) (map[string]*host, int, error) {
	if p.Schema != planSchema || p.ChainID != genesis.ChainID {
		return nil, 0, errors.New("plan schema or chain differs from the engine genesis")
	}
	if len(p.Hosts) == 0 {
		return nil, 0, errors.New("the plan has no hosts")
	}
	byLabel := map[string]*host{}
	ips, keys := map[string]string{}, map[string]string{}
	inGenesis := map[string]bool{}
	for _, v := range genesis.Validators {
		inGenesis[base64.StdEncoding.EncodeToString(v.PubKey.Bytes())] = false
	}
	for i := range p.Hosts {
		h := &p.Hosts[i]
		if !identifier.MatchString(h.Label) || !identifier.MatchString(h.Operator) {
			return nil, 0, fmt.Errorf("host %d: label and operator must be identifiers", i)
		}
		if byLabel[h.Label] != nil {
			return nil, 0, fmt.Errorf("%s: duplicate label", h.Label)
		}
		byLabel[h.Label] = h
		if h.Role != enginepqc.RoleValidator && h.Role != enginepqc.RoleSentry && h.Role != enginepqc.RoleEndpoint {
			return nil, 0, fmt.Errorf("%s: role must be validator, sentry or endpoint", h.Label)
		}
		if !filepath.IsAbs(h.Home) || filepath.Clean(h.Home) != h.Home {
			return nil, 0, fmt.Errorf("%s: home must be a clean absolute path", h.Label)
		}
		ip, port, err := endpoint(h.Label+".p2p", h.P2P)
		if err != nil {
			return nil, 0, err
		}
		// One IP per node (P01, 30 September 2026).
		if other, ok := ips[ip.String()]; ok {
			return nil, 0, fmt.Errorf("%s: shares an IP with %s", h.Label, other)
		}
		ips[ip.String()] = h.Label
		if (h.Role == enginepqc.RoleEndpoint) != (h.Channel != nil) {
			return nil, 0, fmt.Errorf("%s: only an endpoint, and every endpoint, has a channel listener", h.Label)
		}
		if h.Channel != nil {
			channelIP, channelPort, err := endpoint(h.Label+".channel", *h.Channel)
			if err != nil {
				return nil, 0, err
			}
			if !channelIP.Equal(ip) || channelPort == port {
				return nil, 0, fmt.Errorf("%s: the channel listener uses the node's P2P address on another port", h.Label)
			}
		}
		// An endpoint may serve the public status page (P01, 3 October 2026)
		// on the node's address, on a port of its own.
		if h.Status != nil {
			if h.Role != enginepqc.RoleEndpoint {
				return nil, 0, fmt.Errorf("%s: only an endpoint serves the status page", h.Label)
			}
			statusIP, statusPort, err := endpoint(h.Label+".status", *h.Status)
			if err != nil {
				return nil, 0, err
			}
			_, channelPort, _ := endpoint("", *h.Channel)
			if !statusIP.Equal(ip) || statusPort == port || statusPort == channelPort {
				return nil, 0, fmt.Errorf("%s: the status page uses the node's P2P address on its own port", h.Label)
			}
		}
		// Every peer and validator key in the plan is distinct.
		for _, key := range []string{h.PeerPublicKey, h.ValidatorPublicKey} {
			if _, err := publicKey(h.Label, key); err != nil {
				return nil, 0, err
			}
			if other, ok := keys[key]; ok {
				return nil, 0, fmt.Errorf("%s: reuses a key of %s", h.Label, other)
			}
			keys[key] = h.Label
		}
		// No sentry or endpoint key is in the genesis validator set; a
		// validator's may be outside it (P01, 3 October 2026).
		_, member := inGenesis[h.ValidatorPublicKey]
		if member && h.Role != enginepqc.RoleValidator {
			return nil, 0, fmt.Errorf("%s: a sentry or endpoint key may not be in the genesis validator set", h.Label)
		}
		if member {
			inGenesis[h.ValidatorPublicKey] = true
		}
		if h.StateSync != nil && (h.StateSync.TrustHeight < 1 || !trustHash.MatchString(h.StateSync.TrustHash)) {
			return nil, 0, fmt.Errorf("%s: state sync needs a positive trust height and a 32-byte hex trust hash", h.Label)
		}
	}
	unhosted := 0
	for _, placed := range inGenesis {
		if !placed {
			unhosted++
		}
	}
	for i := range p.Hosts {
		h := &p.Hosts[i]
		if len(h.Pins) == 0 || len(h.Pins) > enginepqc.MaxPeers {
			return nil, 0, fmt.Errorf("%s: 1 to %d pins required", h.Label, enginepqc.MaxPeers)
		}
		seen := map[string]bool{}
		for _, label := range h.Pins {
			peer := byLabel[label]
			if peer == nil || label == h.Label || seen[label] {
				return nil, 0, fmt.Errorf("%s: unknown, self or repeated pin %s", h.Label, label)
			}
			seen[label] = true
			// The host firewall admits one address family (render.py).
			if (endpointIP(peer.P2P).To4() == nil) != (endpointIP(h.P2P).To4() == nil) {
				return nil, 0, fmt.Errorf("%s: pins %s across address families", h.Label, label)
			}
			if !contains(peer.Pins, h.Label) {
				return nil, 0, fmt.Errorf("%s pins %s, which does not pin it back", h.Label, label)
			}
			// Validators peer only with their own sentries; endpoints only
			// with sentries (P01, 30 September 2026).
			switch h.Role {
			case enginepqc.RoleValidator:
				if peer.Role != enginepqc.RoleSentry || peer.Operator != h.Operator {
					return nil, 0, fmt.Errorf("%s: a validator pins only its own operator's sentries", h.Label)
				}
			case enginepqc.RoleEndpoint:
				if peer.Role != enginepqc.RoleSentry {
					return nil, 0, fmt.Errorf("%s: an endpoint pins only sentries", h.Label)
				}
			}
		}
	}
	return byLabel, unhosted, nil
}

func endpointIP(address string) net.IP {
	ip, _, _ := endpoint("", address)
	return ip
}

func contains(list []string, value string) bool {
	for _, item := range list {
		if item == value {
			return true
		}
	}
	return false
}

// engineConfig is a host's engine configuration from the approved values.
func engineConfig(h *host, byLabel map[string]*host, v values, genesis *types.GenesisDoc) (*cfg.Config, error) {
	c := cfg.DefaultConfig().SetRoot(h.Home)
	c.Moniker = h.Label
	c.ProxyApp = "unix://" + filepath.Join(h.Home, "abci", "app.sock")
	c.ABCI = "socket"
	c.PrivValidatorListenAddr = ""
	c.DBBackend = "goleveldb"
	c.LogLevel = "info"
	c.LogFormat = "json"
	c.RPC.ListenAddress = rpcPlaceholder
	c.RPC.GRPCListenAddress = ""
	c.RPC.PprofListenAddress = ""
	c.RPC.Unsafe = false
	c.RPC.CORSAllowedOrigins = nil
	c.P2P.ListenAddress = "tcp://" + h.P2P
	c.P2P.ExternalAddress = ""
	c.P2P.Seeds = ""
	c.P2P.PexReactor = false
	c.P2P.SeedMode = false
	c.P2P.AddrBookStrict = true
	c.P2P.AllowDuplicateIP = false
	c.Mempool.Type = cfg.MempoolTypeFlood
	c.Mempool.Recheck = true
	c.Instrumentation.Prometheus = false
	durations := []struct {
		name   string
		text   string
		target *time.Duration
	}{
		{"timeout_propose", v.Consensus.TimeoutPropose, &c.Consensus.TimeoutPropose},
		{"timeout_propose_delta", v.Consensus.TimeoutProposeDelta, &c.Consensus.TimeoutProposeDelta},
		{"timeout_prevote", v.Consensus.TimeoutPrevote, &c.Consensus.TimeoutPrevote},
		{"timeout_prevote_delta", v.Consensus.TimeoutPrevoteDelta, &c.Consensus.TimeoutPrevoteDelta},
		{"timeout_precommit", v.Consensus.TimeoutPrecommit, &c.Consensus.TimeoutPrecommit},
		{"timeout_precommit_delta", v.Consensus.TimeoutPrecommitDelta, &c.Consensus.TimeoutPrecommitDelta},
		{"timeout_commit", v.Consensus.TimeoutCommit, &c.Consensus.TimeoutCommit},
		{"create_empty_blocks_interval", v.Consensus.CreateEmptyBlocksInterval, &c.Consensus.CreateEmptyBlocksInterval},
		{"peer_gossip_sleep_duration", v.Consensus.PeerGossipSleepDuration, &c.Consensus.PeerGossipSleepDuration},
		{"peer_query_maj23_sleep_duration", v.Consensus.PeerQueryMaj23SleepDuration, &c.Consensus.PeerQueryMaj23SleepDuration},
		{"block_time_tolerance", v.Consensus.BlockTimeTolerance, &c.Consensus.BlockTimeTolerance},
		{"recheck_timeout", v.Mempool.RecheckTimeout, &c.Mempool.RecheckTimeout},
		{"flush_throttle_timeout", v.P2P.FlushThrottleTimeout, &c.P2P.FlushThrottleTimeout},
		{"handshake_timeout", v.P2P.HandshakeTimeout, &c.P2P.HandshakeTimeout},
		{"dial_timeout", v.P2P.DialTimeout, &c.P2P.DialTimeout},
		{"persistent_peers_max_dial_period", v.P2P.PersistentPeersMaxDialPeriod, &c.P2P.PersistentPeersMaxDialPeriod},
		{"discovery_time", v.StateSync.DiscoveryTime, &c.StateSync.DiscoveryTime},
		{"chunk_request_timeout", v.StateSync.ChunkRequestTimeout, &c.StateSync.ChunkRequestTimeout},
		{"trust_period", v.StateSync.TrustPeriod, &c.StateSync.TrustPeriod},
	}
	for _, d := range durations {
		value, err := duration(d.name, d.text)
		if err != nil {
			return nil, err
		}
		*d.target = value
	}
	c.Consensus.SkipTimeoutCommit = v.Consensus.SkipTimeoutCommit
	c.Consensus.CreateEmptyBlocks = v.Consensus.CreateEmptyBlocks
	check, ok := v.Consensus.DoubleSignCheckHeight[h.Role]
	if !ok || check < 0 {
		return nil, fmt.Errorf("double_sign_check_height for %s missing", h.Role)
	}
	c.Consensus.DoubleSignCheckHeight = check
	c.Mempool.Size = v.Mempool.Size
	c.Mempool.MaxTxsBytes = v.Mempool.MaxTxsBytes
	c.Mempool.CacheSize = v.Mempool.CacheSize
	c.Mempool.MaxTxBytes = v.Mempool.MaxTxBytes
	c.P2P.MaxNumInboundPeers = v.P2P.MaxNumInboundPeers
	c.P2P.MaxNumOutboundPeers = v.P2P.MaxNumOutboundPeers
	c.P2P.MaxPacketMsgPayloadSize = v.P2P.MaxPacketMsgPayloadSize
	c.P2P.SendRate = v.P2P.SendRate
	c.P2P.RecvRate = v.P2P.RecvRate
	c.StateSync.ChunkFetchers = v.StateSync.ChunkFetchers
	c.StateSync.MaxSnapshotChunks = v.StateSync.MaxSnapshotChunks
	c.StateSync.RPCServers = nil
	if h.StateSync != nil {
		// State sync from the operator's light blocks (state sync v1); the
		// trust period stays below the evidence age, as the engine requires.
		if c.StateSync.TrustPeriod >= genesis.ConsensusParams.Evidence.MaxAgeDuration {
			return nil, errors.New("state sync trust period must be below the evidence age")
		}
		c.StateSync.Enable = true
		c.StateSync.TrustHeight = h.StateSync.TrustHeight
		c.StateSync.TrustHash = strings.ToUpper(h.StateSync.TrustHash)
	} else {
		c.StateSync.Enable = false
	}
	var pins []string
	for _, label := range h.Pins {
		peer := byLabel[label]
		raw, _ := publicKey(label, peer.PeerPublicKey)
		key, _ := mldsa65.NewPubKeyFromBytes(raw)
		pins = append(pins, string(p2p.PubKeyToID(key))+"@"+peer.P2P)
	}
	c.P2P.PersistentPeers = strings.Join(pins, ",")
	return c, nil
}

// transport is a host's canonical PQC transport file.
func transport(h *host, byLabel map[string]*host, v values, chain string) (enginepqc.TransportConfig, []byte, error) {
	tc := enginepqc.TransportConfig{Version: 1, Profile: enginepqc.ProductionProfile, Network: chain,
		LocalPublicKeyBase64: h.PeerPublicKey, HandshakeTimeoutMS: v.Transport.HandshakeTimeoutMS}
	for _, label := range h.Pins {
		peer := byLabel[label]
		raw, _ := publicKey(label, peer.PeerPublicKey)
		key, _ := mldsa65.NewPubKeyFromBytes(raw)
		tc.Peers = append(tc.Peers, enginepqc.PeerPin{ID: string(p2p.PubKeyToID(key)), PublicKeyBase64: peer.PeerPublicKey, Address: peer.P2P})
	}
	raw, err := json.Marshal(tc)
	return tc, raw, err
}

// renderConfig writes the engine's own template, then reads it back with the
// engine's own decoder, so the host gets exactly what it will parse.
func renderConfig(c *cfg.Config, scratch string) ([]byte, *cfg.Config, error) {
	path := filepath.Join(scratch, "config.toml")
	cfg.WriteConfigFile(path, c)
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, nil, err
	}
	if err := os.Remove(path); err != nil {
		return nil, nil, err
	}
	decoded, err := enginepqc.DecodeConfig(raw, c.RootDir)
	return raw, decoded, err
}

// generate builds every host's files and the published bindings.
func generate(planRaw, valuesRaw, genesisRaw []byte, scratch string) ([]output, error) {
	var p plan
	if err := complete(planRaw, &p); err != nil {
		return nil, fmt.Errorf("pin plan: %w", err)
	}
	var v values
	if err := complete(valuesRaw, &v); err != nil {
		return nil, fmt.Errorf("host values: %w", err)
	}
	roles := v.Consensus.DoubleSignCheckHeight
	_, validator := roles[enginepqc.RoleValidator]
	_, sentry := roles[enginepqc.RoleSentry]
	_, endpoint := roles[enginepqc.RoleEndpoint]
	if v.Schema != valuesSchema || len(roles) != 3 || !validator || !sentry || !endpoint {
		return nil, errors.New("unsupported host values schema or roles")
	}
	genesis, err := types.GenesisDocFromJSON(genesisRaw)
	if err != nil {
		return nil, fmt.Errorf("engine genesis: %w", err)
	}
	byLabel, unhosted, err := checkPlan(p, genesis)
	if err != nil {
		return nil, err
	}
	labels := make([]string, 0, len(byLabel))
	for label := range byLabel {
		labels = append(labels, label)
	}
	sort.Strings(labels)
	var files []output
	summary := bindings{Schema: bindingsSchema, ChainID: p.ChainID, PlanSHA256: digest(planRaw),
		ValuesSHA256: digest(valuesRaw), GenesisSHA256: digest(genesisRaw), GenesisValidatorsWithoutHost: unhosted}
	for _, label := range labels {
		h := byLabel[label]
		c, err := engineConfig(h, byLabel, v, genesis)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", label, err)
		}
		configRaw, decoded, err := renderConfig(c, scratch)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", label, err)
		}
		tc, transportRaw, err := transport(h, byLabel, v, p.ChainID)
		if err != nil {
			return nil, err
		}
		peerRaw, _ := publicKey(label, h.PeerPublicKey)
		validatorRaw, _ := publicKey(label, h.ValidatorPublicKey)
		// The engine's own start checks for the production profile.
		if err := enginepqc.ValidatePublicHost(decoded, genesis, tc, peerRaw, validatorRaw, h.Role, enginepqc.ProductionProfile); err != nil {
			return nil, fmt.Errorf("%s: %w", label, err)
		}
		binding := enginepqc.ProductionBinding{Schema: 1, Role: h.Role, ChainID: p.ChainID,
			ConfigSHA256: digest(configRaw), GenesisSHA256: digest(genesisRaw), TransportSHA256: digest(transportRaw),
			PeerPublicKeySHA256: digest(peerRaw), ValidatorPublicKeySHA256: digest(validatorRaw)}
		bindingRaw, err := json.Marshal(binding)
		if err != nil {
			return nil, err
		}
		if _, err := enginepqc.DecodeProductionBinding(bindingRaw); err != nil {
			return nil, err
		}
		dir := filepath.Join("hosts", label)
		files = append(files, output{filepath.Join(dir, "config.toml"), configRaw},
			output{filepath.Join(dir, "pqc_transport.json"), transportRaw},
			output{filepath.Join(dir, "binding.json"), bindingRaw})
		listeners := []string{}
		if h.Channel != nil {
			listeners = append(listeners, *h.Channel)
		}
		if h.Status != nil {
			listeners = append(listeners, *h.Status)
		}
		summary.Hosts = append(summary.Hosts, hostBinding{Label: label, Operator: h.Operator, Role: h.Role,
			Pins: append([]string(nil), h.Pins...), Firewall: firewall{binding.TransportSHA256, h.P2P, listeners},
			Binding: binding, BindingSHA256: digest(bindingRaw)})
	}
	summaryRaw, err := json.MarshalIndent(summary, "", "  ")
	if err != nil {
		return nil, err
	}
	files = append(files, output{"PIN_PLAN_BINDINGS.json", append(summaryRaw, '\n')})
	return files, nil
}

func run(args []string, stdout io.Writer) error {
	flags := flag.NewFlagSet("dytallix-host-config", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	planPath := flags.String("plan", "", "the public pin plan")
	valuesPath := flags.String("values", "", "the resolved host values")
	genesisPath := flags.String("genesis", "", "the engine genesis")
	out := flags.String("out", "", "a new output directory")
	if err := flags.Parse(args); err != nil || flags.NArg() != 0 || *planPath == "" || *valuesPath == "" || *genesisPath == "" || *out == "" {
		return errors.New("usage: dytallix-host-config --plan PIN_PLAN.json --values HOST_VALUES.json --genesis ENGINE_GENESIS --out DIR")
	}
	var inputs [3][]byte
	for i, path := range []string{*planPath, *valuesPath, *genesisPath} {
		raw, err := readBounded(path)
		if err != nil {
			return err
		}
		inputs[i] = raw
	}
	scratch, err := os.MkdirTemp("", "dytallix-host-config-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(scratch)
	files, err := generate(inputs[0], inputs[1], inputs[2], scratch)
	if err != nil {
		return err
	}
	// Nothing is written unless every host passed.
	if err := os.Mkdir(*out, 0o755); err != nil {
		return err
	}
	for _, file := range files {
		path := filepath.Join(*out, file.path)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
		// The engine reads its files owner-only.
		if err := os.WriteFile(path, file.data, 0o600); err != nil {
			return err
		}
	}
	fmt.Fprintf(stdout, "{\"status\":\"GENERATED\",\"hosts\":%d,\"bindings_sha256\":%q}\n", (len(files)-1)/3, digest(files[len(files)-1].data))
	return nil
}

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
