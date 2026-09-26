package enginepqc

import (
	"encoding/base64"
	"net"
	"path/filepath"
	"testing"
	"time"

	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func isolatedConfig(t *testing.T) *cfg.Config {
	t.Helper()
	c := cfg.DefaultConfig().SetRoot(t.TempDir())
	c.P2P.LibP2PConfig.Enabled = false
	c.P2P.PexReactor = false
	c.P2P.SeedMode = false
	c.P2P.Seeds = ""
	c.P2P.ExternalAddress = ""
	c.P2P.ListenAddress = "tcp://127.0.0.1:31000"
	c.RPC.ListenAddress = "tcp://127.0.0.1:31001"
	c.ProxyApp = "unix://" + filepath.Join(c.RootDir, "abci", "app.sock")
	c.ABCI = "socket"
	c.PrivValidatorListenAddr = ""
	c.StateSync.Enable = false
	c.RPC.GRPCListenAddress = ""
	c.RPC.PprofListenAddress = ""
	c.RPC.Unsafe = false
	c.Instrumentation.Prometheus = false
	return c
}
func TestIsolationRejectsAlternativeNetworkAndSignerPaths(t *testing.T) {
	changes := map[string]func(*cfg.Config){
		"remote signer":       func(c *cfg.Config) { c.PrivValidatorListenAddr = "tcp://127.0.0.1:31002" },
		"libp2p":              func(c *cfg.Config) { c.P2P.LibP2PConfig.Enabled = true },
		"discovery":           func(c *cfg.Config) { c.P2P.PexReactor = true },
		"state sync":          func(c *cfg.Config) { c.StateSync.Enable = true },
		"public P2P":          func(c *cfg.Config) { c.P2P.ListenAddress = "tcp://0.0.0.0:31000" },
		"public RPC":          func(c *cfg.Config) { c.RPC.ListenAddress = "tcp://0.0.0.0:31001" },
		"TCP ABCI":            func(c *cfg.Config) { c.ProxyApp = "tcp://127.0.0.1:31002" },
		"unsafe RPC":          func(c *cfg.Config) { c.RPC.Unsafe = true },
		"TLS fallback":        func(c *cfg.Config) { c.RPC.TLSCertFile = "certificate.pem"; c.RPC.TLSKeyFile = "key.pem" },
		"wildcard CORS":       func(c *cfg.Config) { c.RPC.CORSAllowedOrigins = []string{"*"} },
		"public CORS":         func(c *cfg.Config) { c.RPC.CORSAllowedOrigins = []string{"https://example.com"} },
		"other local CORS":    func(c *cfg.Config) { c.RPC.CORSAllowedOrigins = []string{"http://127.0.0.1:4174"} },
		"unbounded peers":     func(c *cfg.Config) { c.P2P.MaxNumInboundPeers = MaxPeers + 1 },
		"unconditional peers": func(c *cfg.Config) { c.P2P.UnconditionalPeerIDs = "override" },
	}
	if err := ValidateIsolation(isolatedConfig(t)); err != nil {
		t.Fatal(err)
	}
	browser := isolatedConfig(t)
	browser.RPC.CORSAllowedOrigins = []string{"http://127.0.0.1:4173"}
	if err := ValidateIsolation(browser); err != nil {
		t.Fatal(err)
	}
	for name, change := range changes {
		t.Run(name, func(t *testing.T) {
			c := isolatedConfig(t)
			change(c)
			if ValidateIsolation(c) == nil {
				t.Fatal("unsupported path accepted")
			}
		})
	}
}
func testKey(t *testing.T) *p2p.NodeKey {
	t.Helper()
	private, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	return &p2p.NodeKey{PrivKey: private}
}
func TestFullPinMatchesPeerIDAddressAndPersistentSet(t *testing.T) {
	local, remote, other := testKey(t), testKey(t), testKey(t)
	address := "127.0.0.1:31010"
	c := isolatedConfig(t)
	c.P2P.PersistentPeers = string(remote.ID()) + "@" + address
	tc := TransportConfig{Version: 1, Profile: Profile, Network: "pqc-fixture", LocalPublicKeyBase64: base64.StdEncoding.EncodeToString(local.PubKey().Bytes()), Peers: []PeerPin{{string(remote.ID()), base64.StdEncoding.EncodeToString(remote.PubKey().Bytes()), address}}, HandshakeTimeoutMS: 5000}
	if pins, _, _, err := validatePins(tc, c, local, "pqc-fixture"); err != nil || len(pins) != 1 {
		t.Fatalf("valid pins rejected: %v", err)
	}
	cases := map[string]func(*TransportConfig){
		"different full key": func(v *TransportConfig) {
			v.Peers[0].PublicKeyBase64 = base64.StdEncoding.EncodeToString(other.PubKey().Bytes())
		},
		"short hash pin": func(v *TransportConfig) {
			v.Peers[0].PublicKeyBase64 = base64.StdEncoding.EncodeToString(remote.PubKey().Address())
		},
		"different address": func(v *TransportConfig) { v.Peers[0].Address = "127.0.0.1:31011" },
		"public address":    func(v *TransportConfig) { v.Peers[0].Address = "192.0.2.1:31010" },
		"different network": func(v *TransportConfig) { v.Network = "another-fixture" },
		"different local key": func(v *TransportConfig) {
			v.LocalPublicKeyBase64 = base64.StdEncoding.EncodeToString(other.PubKey().Bytes())
		},
		"duplicate peer":      func(v *TransportConfig) { v.Peers = append(v.Peers, v.Peers[0]) },
		"unsupported profile": func(v *TransportConfig) { v.Profile = "legacy" },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			copy := tc
			copy.Peers = append([]PeerPin(nil), tc.Peers...)
			change(&copy)
			if _, _, _, err := validatePins(copy, c, local, "pqc-fixture"); err == nil {
				t.Fatal("unbound pin accepted")
			}
		})
	}
}
func TestHandshakeCapacityRejectsBeforeIOAndReleasesOnPrecheckError(t *testing.T) {
	runtime := &Runtime{slots: make(chan struct{}, MaxConcurrentHandshakes), byID: make(map[p2p.ID][]byte)}
	for i := 0; i < MaxConcurrentHandshakes; i++ {
		runtime.slots <- struct{}{}
	}
	// nil is safe here only because the full capacity must reject before any I/O.
	if _, err := runtime.Upgrade(log.NewNopLogger())(nil, nil, time.Second); err == nil {
		t.Fatal("full handshake capacity accepted")
	}
	for i := 0; i < MaxConcurrentHandshakes; i++ {
		<-runtime.slots
	}
	left, right := net.Pipe()
	defer left.Close()
	defer right.Close()
	if _, err := runtime.Upgrade(log.NewNopLogger())(left, &p2p.NetAddress{ID: "not-configured"}, time.Second); err == nil {
		t.Fatal("unconfigured outbound peer accepted")
	}
	if len(runtime.slots) != 0 {
		t.Fatal("failed precheck leaked handshake slot")
	}
}
