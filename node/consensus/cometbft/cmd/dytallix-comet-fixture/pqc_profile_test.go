package main

import (
	cfg "github.com/cometbft/cometbft/config"
	"testing"
)

func TestTransportProfileRefusesProductionAndUnimplementedPQC(t *testing.T) {
	for _, tc := range []struct {
		profile    string
		production bool
		allow      bool
	}{
		{legacyLoopbackTransport, false, true},
		{legacyLoopbackTransport, true, false},
		{pqcLoopbackTransport, false, true},
		{pqcSeedLoopbackTransport, false, true},
		{pqcSeedLoopbackTransport, true, false},
		{pqcPrivateSeedTransport, false, true},
		{pqcPrivateSeedTransport, true, false},
		{"mlkem768-mldsa65", false, false},
		{"", false, false},
		{"production", true, false},
	} {
		err := checkTransportProfile(tc.profile, tc.production)
		if (err == nil) != tc.allow {
			t.Fatalf("profile %q production %v: %v", tc.profile, tc.production, err)
		}
	}
}

func TestLegacyIsolationRejectsBoundaryExpansion(t *testing.T) {
	base := func() *cfg.Config {
		c := cfg.DefaultConfig()
		c.PrivValidatorListenAddr = ""
		c.P2P.LibP2PConfig.Enabled = false
		c.P2P.ListenAddress = "tcp://127.0.0.1:20000"
		c.RPC.ListenAddress = "tcp://127.0.0.1:20001"
		c.ProxyApp = "unix:///tmp/fixture.sock"
		c.P2P.PexReactor = false
		return c
	}
	if e := validateLegacyIsolation(base()); e != nil {
		t.Fatal(e)
	}
	cases := map[string]func(*cfg.Config){
		"public listener": func(c *cfg.Config) { c.P2P.ListenAddress = "tcp://0.0.0.0:20000" },
		"public RPC":      func(c *cfg.Config) { c.RPC.ListenAddress = "tcp://0.0.0.0:20001" },
		"remote signer":   func(c *cfg.Config) { c.PrivValidatorListenAddr = "tcp://127.0.0.1:20002" },
		"libp2p":          func(c *cfg.Config) { c.P2P.LibP2PConfig.Enabled = true },
		"discovery":       func(c *cfg.Config) { c.P2P.PexReactor = true },
		"remote peer":     func(c *cfg.Config) { c.P2P.PersistentPeers = "peer@192.0.2.1:20000" },
		"DNS peer":        func(c *cfg.Config) { c.P2P.PersistentPeers = "peer@localhost:20000" },
		"ABCI TCP":        func(c *cfg.Config) { c.ProxyApp = "tcp://127.0.0.1:20003" },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			c := base()
			change(c)
			if validateLegacyIsolation(c) == nil {
				t.Fatal("expanded trust boundary accepted")
			}
		})
	}
}
