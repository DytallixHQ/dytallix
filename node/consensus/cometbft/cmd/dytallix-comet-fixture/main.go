// Generates disposable, loopback-only qualification files. It creates no mainnet keys.
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
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/pqcp2p"

	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/types"
)

type appValidator struct {
	PubkeyType    string `json:"pubkey_type"`
	PubkeyBase64  string `json:"pubkey_base64"`
	Power         int64  `json:"power"`
	RewardAddress string `json:"reward_address"`
}
type applicationConfig struct {
	Profile        string           `json:"profile"`
	Engine         string           `json:"engine"`
	ChainID        string           `json:"chain_id"`
	AppStateSHA256 string           `json:"app_state_sha256"`
	GasPrice       uint64           `json:"gas_price"`
	MaxTxBytes     uint64           `json:"max_tx_bytes"`
	MaxBlockBytes  uint64           `json:"max_block_bytes"`
	MaxTxs         uint64           `json:"max_txs"`
	Validators     []appValidator   `json:"validators"`
	Lifecycle      *lifecycleConfig `json:"lifecycle,omitempty"`
	Penalty        *penaltyConfig   `json:"penalty,omitempty"`
}
type nodeSummary struct {
	Home   string `json:"home"`
	Socket string `json:"socket"`
	RPC    string `json:"rpc"`
	P2P    string `json:"p2p"`
}

// embedGenesis keeps the application bytes exact. The app hash binds these bytes.
// MarshalIndent would change RawMessage whitespace and break that binding.
func embedGenesis(doc *types.GenesisDoc, app []byte) ([]byte, error) {
	copyDoc := *doc
	copyDoc.AppState = nil
	encoded, err := cmtjson.Marshal(copyDoc)
	if err != nil {
		return nil, err
	}
	if len(encoded) == 0 || encoded[len(encoded)-1] != '}' {
		return nil, errors.New("invalid engine genesis encoding")
	}
	encoded = append(encoded[:len(encoded)-1], []byte(",\"app_state\":")...)
	encoded = append(encoded, app...)
	encoded = append(encoded, '}')
	var check struct {
		AppState json.RawMessage `json:"app_state"`
	}
	if err := json.Unmarshal(encoded, &check); err != nil {
		return nil, err
	}
	if !bytes.Equal(check.AppState, app) {
		return nil, errors.New("engine genesis changed application bytes")
	}
	parsed, err := types.GenesisDocFromJSON(encoded)
	if err != nil {
		return nil, err
	}
	if !bytes.Equal(parsed.AppState, app) {
		return nil, errors.New("official engine decoder changed application bytes")
	}
	return encoded, nil
}

func generate(output, appFile, chainID, genesisTime string, basePort int) ([]nodeSummary, error) {
	return generateWithLifecycle(output, appFile, chainID, genesisTime, basePort, "")
}
func generateWithLifecycle(output, appFile, chainID, genesisTime string, basePort int, operatorsFile string) ([]nodeSummary, error) {
	return generateWithPenalty(output, appFile, chainID, genesisTime, basePort, operatorsFile, false)
}
func generateWithPenalty(output, appFile, chainID, genesisTime string, basePort int, operatorsFile string, penalty bool) ([]nodeSummary, error) {
	return generateWithTransport(output, appFile, chainID, genesisTime, basePort, operatorsFile, penalty, legacyLoopbackTransport)
}
func generateWithTransport(output, appFile, chainID, genesisTime string, basePort int, operatorsFile string, penalty bool, profile string) ([]nodeSummary, error) {
	return generateWithTransportIPs(output, appFile, chainID, genesisTime, basePort, operatorsFile, penalty, profile, nil)
}

func generateWithTransportIPs(output, appFile, chainID, genesisTime string, basePort int, operatorsFile string, penalty bool, profile string, peerIPs []string) ([]nodeSummary, error) {
	if err := checkTransportProfile(profile, false); err != nil {
		return nil, err
	}
	if penalty && operatorsFile == "" {
		return nil, errors.New("penalty fixture requires lifecycle operators")
	}
	nodeCount := 4
	if operatorsFile != "" {
		nodeCount = 6
	}
	if profile == pqcPrivateSeedTransport {
		if len(peerIPs) != nodeCount {
			return nil, errors.New("private PQC fixture requires one explicit IP for each node")
		}
		seen := make(map[string]bool, nodeCount)
		for _, text := range peerIPs {
			ip := net.ParseIP(text)
			if ip == nil || !ip.IsPrivate() || ip.String() != text || seen[text] {
				return nil, errors.New("private PQC fixture requires distinct canonical private IP literals")
			}
			seen[text] = true
		}
	} else if len(peerIPs) != 0 {
		return nil, errors.New("peer IP list is only valid for the private PQC fixture")
	}

	if !filepath.IsAbs(output) || filepath.Clean(output) != output {
		return nil, errors.New("output must be an absolute clean path")
	}
	if basePort < 1024 || basePort > 65535-(nodeCount-1)*10-1 {
		return nil, errors.New("base port is outside the fixture port range")
	}
	if chainID == "" || len(chainID) > types.MaxChainIDLen {
		return nil, errors.New("supply a valid explicit chain ID")
	}
	if profile == pqcPrivateSeedTransport && (strings.Contains(strings.ToLower(chainID), "mainnet") || strings.Contains(strings.ToLower(chainID), "production")) {
		return nil, errors.New("private PQC fixture requires a nonproduction chain ID")
	}
	timestamp, err := time.Parse(time.RFC3339Nano, genesisTime)
	if err != nil || timestamp.Unix() < 0 {
		return nil, errors.New("supply an explicit nonnegative RFC3339 genesis time")
	}
	app, err := os.ReadFile(appFile)
	if err != nil {
		return nil, err
	}
	if len(app) == 0 || len(app) > 1<<20 {
		return nil, errors.New("native fixture genesis must be between 1 byte and 1 MiB")
	}
	var compact bytes.Buffer
	if err := json.Compact(&compact, app); err != nil {
		return nil, err
	}
	if !bytes.Equal(compact.Bytes(), app) {
		return nil, errors.New("native fixture genesis must be compact JSON without a trailing newline")
	}
	var identity struct {
		ChainID string `json:"chain_id"`
	}
	if err := json.Unmarshal(app, &identity); err != nil {
		return nil, err
	}
	if identity.ChainID != chainID {
		return nil, errors.New("native genesis chain ID differs from requested chain ID")
	}
	for i := 0; i < nodeCount; i++ {
		if len(filepath.Join(output, fmt.Sprintf("node%d", i), "abci", "app.sock")) > 100 {
			return nil, errors.New("use a shorter output path for portable Unix socket paths")
		}
	}
	var lifecycle *lifecycleConfig
	powers := []int64{10, 10, 10, 10}
	if operatorsFile != "" {
		lifecycle, powers, err = fixtureLifecycle(operatorsFile, app, chainID)
		if err != nil {
			return nil, err
		}
	}
	// Refuse to reuse any directory or key. Keep partial output for diagnosis on error.
	if err := os.Mkdir(output, 0o700); err != nil {
		return nil, fmt.Errorf("create new fixture directory: %w", err)
	}
	genVals := make([]types.GenesisValidator, 4)
	appVals := make([]appValidator, 4)
	configs := make([]*cfg.Config, nodeCount)
	peers := make([]string, nodeCount)
	peerPublicKeys := make([][]byte, nodeCount)
	nodes := make([]nodeSummary, nodeCount)
	publicKeys := make([]map[string]any, nodeCount)
	for i := 0; i < nodeCount; i++ {
		home := filepath.Join(output, fmt.Sprintf("node%d", i))
		for _, dir := range []string{home, filepath.Join(home, "config"), filepath.Join(home, "data"), filepath.Join(home, "abci")} {
			if err := os.Mkdir(dir, 0o700); err != nil {
				return nil, err
			}
		}
		config := cfg.DefaultConfig().SetRoot(home)
		config.Moniker = fmt.Sprintf("dytallix-local-%d", i)
		config.ProxyApp = "unix://" + filepath.Join(home, "abci", "app.sock")
		config.ABCI = "socket"
		config.PrivValidatorListenAddr = ""
		config.P2P.LibP2PConfig.Enabled = false
		config.DBBackend = "goleveldb"
		config.LogLevel = "info"
		config.LogFormat = "json"
		config.RPC.ListenAddress = fmt.Sprintf("tcp://127.0.0.1:%d", basePort+i*10+1)
		config.RPC.GRPCListenAddress = ""
		config.RPC.PprofListenAddress = ""
		config.RPC.Unsafe = false
		config.P2P.ListenAddress = fmt.Sprintf("tcp://127.0.0.1:%d", basePort+i*10)
		if profile == pqcPrivateSeedTransport {
			config.P2P.ListenAddress = "tcp://" + net.JoinHostPort(peerIPs[i], strconv.Itoa(basePort+i*10))
			config.P2P.AddrBookStrict = true
		}
		config.P2P.ExternalAddress = ""
		config.P2P.Seeds = ""
		if profile != pqcPrivateSeedTransport {
			config.P2P.AddrBookStrict = false
			config.P2P.AllowDuplicateIP = true
		}
		config.P2P.PexReactor = false
		config.P2P.SeedMode = false
		config.Mempool.Type = "flood"
		config.Consensus.CreateEmptyBlocks = true
		config.Consensus.TimeoutCommit = time.Second
		config.StateSync.Enable = false
		config.Instrumentation.Prometheus = false
		if err := config.ValidateBasic(); err != nil {
			return nil, err
		}
		key, err := mldsa65.GenPrivKey()
		if err != nil {
			return nil, err
		}
		pv := privval.NewFilePV(key, config.PrivValidatorKeyFile(), config.PrivValidatorStateFile())
		pv.Save()
		pubkey, err := pv.GetPubKey()
		if err != nil {
			return nil, err
		}
		publicKeys[i] = map[string]any{"node": i, "pubkey_type": mldsa65.KeyType, "pubkey_base64": base64.StdEncoding.EncodeToString(pubkey.Bytes()), "consensus_address": hex.EncodeToString(pubkey.Address())}
		if i < 4 {
			genVals[i] = types.GenesisValidator{Address: pubkey.Address(), PubKey: pubkey, Power: powers[i], Name: fmt.Sprintf("validator-%d", i)}
			appVals[i] = appValidator{mldsa65.KeyType, base64.StdEncoding.EncodeToString(pubkey.Bytes()), powers[i], fmt.Sprintf("validator-%d", i)}
		}
		nodeKey, err := fixtureNodeKey(config.NodeKeyFile(), profile)
		if err != nil {
			return nil, err
		}
		peerPublicKeys[i] = append([]byte(nil), nodeKey.PubKey().Bytes()...)
		peerAddress := fmt.Sprintf("127.0.0.1:%d", basePort+i*10)
		if profile == pqcPrivateSeedTransport {
			peerAddress = net.JoinHostPort(peerIPs[i], strconv.Itoa(basePort+i*10))
		}
		peers[i] = fmt.Sprintf("%s@%s", nodeKey.ID(), peerAddress)
		configs[i] = config
		nodes[i] = nodeSummary{home, config.ProxyApp, fmt.Sprintf("http://127.0.0.1:%d", basePort+i*10+1), config.P2P.ListenAddress}
	}
	params := types.DefaultConsensusParams()
	params.Validator.PubKeyTypes = []string{mldsa65.KeyType}
	params.Block.MaxBytes = 1048576
	params.Block.MaxGas = -1
	params.Evidence.MaxBytes = 65536
	params.ABCI.VoteExtensionsEnableHeight = 0
	if lifecycle != nil {
		params.Evidence.MaxAgeNumBlocks = lifecycle.EvidenceMaxAgeBlocks
		params.Evidence.MaxAgeDuration = time.Duration(lifecycle.EvidenceMaxAgeSeconds) * time.Second
	}
	doc := &types.GenesisDoc{GenesisTime: timestamp, ChainID: chainID, InitialHeight: 1, ConsensusParams: params, Validators: genVals}
	genesis, err := embedGenesis(doc, app)
	if err != nil {
		return nil, err
	}
	for i, config := range configs {
		other := make([]string, 0, nodeCount-1)
		for j, peer := range peers {
			if i != j {
				other = append(other, peer)
			}
		}
		config.P2P.PersistentPeers = strings.Join(other, ",")
		if profile == legacyLoopbackTransport {
			if err := validateLegacyIsolation(config); err != nil {
				return nil, err
			}
		} else if err := enginepqc.ValidateProfileIsolation(config, profile); err != nil {
			return nil, err
		}
		if profile == pqcLoopbackTransport || profile == pqcSeedLoopbackTransport || profile == pqcPrivateSeedTransport {
			if err := writePQCTransportFixture(config.RootDir, chainID, i, peerPublicKeys, basePort, profile, peerIPs); err != nil {
				return nil, err
			}
		}
		cfg.WriteConfigFile(filepath.Join(config.RootDir, "config", "config.toml"), config)
		if err := os.WriteFile(config.GenesisFile(), genesis, 0o600); err != nil {
			return nil, err
		}
	}
	hash := sha256.Sum256(app)
	application := applicationConfig{Profile: "cometbft-local-qualification", Engine: "cometbft-v0.40.0", ChainID: chainID, AppStateSHA256: hex.EncodeToString(hash[:]), GasPrice: 1, MaxTxBytes: 262144, MaxBlockBytes: 1048576, MaxTxs: 1000, Validators: appVals, Lifecycle: lifecycle}
	if lifecycle != nil {
		application.Profile = lifecycle.Profile
	}
	if penalty {
		application.Profile = "cometbft-penalty-local-qualification"
		application.Penalty = fixturePenalty(chainID)
	}
	if lifecycle != nil {
		publicJSON, err := json.MarshalIndent(publicKeys, "", "  ")
		if err != nil {
			return nil, err
		}
		if err := os.WriteFile(filepath.Join(output, "lifecycle-public-keys.json"), append(publicJSON, '\n'), 0o600); err != nil {
			return nil, err
		}
	}
	encoded, err := json.MarshalIndent(application, "", "  ")
	if err != nil {
		return nil, err
	}
	if err := os.WriteFile(filepath.Join(output, "application-config.json"), append(encoded, '\n'), 0o600); err != nil {
		return nil, err
	}
	if err := os.WriteFile(filepath.Join(output, "native-genesis.json"), app, 0o600); err != nil {
		return nil, err
	}
	if err := filepath.WalkDir(output, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return os.Chmod(path, 0o700)
		}
		return os.Chmod(path, 0o600)
	}); err != nil {
		return nil, err
	}
	return nodes, nil
}

const legacyLoopbackTransport = "legacy-cometbft-loopback-only"

func checkTransportProfile(profile string, production bool) error {
	if production || (profile != legacyLoopbackTransport && profile != pqcLoopbackTransport && profile != pqcSeedLoopbackTransport && profile != pqcPrivateSeedTransport) {
		return pqcp2p.RequireProductionTransport()
	}
	return nil
}

func run() error {
	production := flag.Bool("production", false, "production is blocked; this flag always refuses activation")
	p2pProfile := flag.String("p2p-profile", legacyLoopbackTransport, "explicit legacy, PQC loopback, or private-network PQC seed-backed development transport")
	peerIPs := flag.String("peer-ips", "", "comma-separated canonical private IP literals; required only for the private PQC fixture")
	output := flag.String("output", "", "new absolute fixture directory")
	app := flag.String("app-genesis", "", "compact native development genesis JSON")
	chain := flag.String("chain-id", "", "explicit fixture chain ID")
	genesisTime := flag.String("genesis-time", "", "explicit RFC3339 fixture genesis time")
	basePort := flag.Int("base-port", 28650, "first loopback P2P port; nodes use ten-port spacing")
	lifecycle := flag.Bool("lifecycle", false, "create six disposable nodes, with four initial validators")
	penalty := flag.Bool("penalty", false, "enable synthetic duplicate-vote penalty qualification; requires --lifecycle")
	operators := flag.String("operators", "", "JSON map from approved fixture validator IDs to funded operator accounts")
	flag.Parse()
	if err := checkTransportProfile(*p2pProfile, *production); err != nil {
		return err
	}
	if flag.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	if *lifecycle != (*operators != "") {
		return errors.New("--lifecycle and --operators must be supplied together")
	}
	var addresses []string
	if *peerIPs != "" {
		addresses = strings.Split(*peerIPs, ",")
	}
	nodes, err := generateWithTransportIPs(*output, *app, *chain, *genesisTime, *basePort, *operators, *penalty, *p2pProfile, addresses)
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{"profile": "local-development-qualification", "production_qualified": false, "launch_status": "NO GO", "p2p_profile": *p2pProfile, "pqc_p2p_integrated": *p2pProfile == pqcLoopbackTransport || *p2pProfile == pqcSeedLoopbackTransport || *p2pProfile == pqcPrivateSeedTransport, "production": false, "nodes": nodes, "application_config": filepath.Join(*output, "application-config.json"), "native_genesis": filepath.Join(*output, "native-genesis.json")})
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
