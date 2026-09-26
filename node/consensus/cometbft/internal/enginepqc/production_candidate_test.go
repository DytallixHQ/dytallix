//go:build dytallix_pqc_only && dytallix_pqc_ipc

package enginepqc

import (
	"bytes"
	"encoding/base64"
	"net"
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func productionCandidateFixture(t *testing.T) (*Runtime, *p2p.NodeKey) {
	t.Helper()
	local, remote := testKey(t), testKey(t)
	c := isolatedConfig(t)
	c.P2P.ListenAddress = "tcp://10.1.2.1:31000"
	c.P2P.AllowDuplicateIP = false
	c.P2P.AddrBookStrict = true
	c.RPC.CORSAllowedOrigins = nil
	c.P2P.PersistentPeers = string(remote.ID()) + "@10.1.2.2:31000"
	tc := TransportConfig{
		Version: 1, Profile: ProductionCandidateProfile, Network: "proposed-chain",
		LocalPublicKeyBase64: base64.StdEncoding.EncodeToString(local.PubKey().Bytes()),
		Peers:                []PeerPin{{ID: string(remote.ID()), PublicKeyBase64: base64.StdEncoding.EncodeToString(remote.PubKey().Bytes()), Address: "10.1.2.2:31000"}},
		HandshakeTimeoutMS:   5000,
	}
	return &Runtime{Config: c, Transport: tc}, local
}

func TestProductionCandidateBindsLivePeerSourceAndKey(t *testing.T) {
	seedA := bytes.Repeat([]byte{1}, mldsa65.SeedSize)
	seedB := bytes.Repeat([]byte{2}, mldsa65.SeedSize)
	privateA, err := mldsa65.GenPrivKeyFromSeed(seedA)
	if err != nil {
		t.Fatal(err)
	}
	privateB, err := mldsa65.GenPrivKeyFromSeed(seedB)
	if err != nil {
		t.Fatal(err)
	}
	keyA := &p2p.NodeKey{PrivKey: privateA}
	keyB := &p2p.NodeKey{PrivKey: privateB}
	a := testPrivateRuntime(t, 1, "9.9.9.9:31000", "8.8.8.8:31000", keyB)
	b := testPrivateRuntime(t, 2, "8.8.8.8:31000", "9.9.9.9:31000", keyA)
	a.Transport.Profile = ProductionCandidateProfile
	b.Transport.Profile = ProductionCandidateProfile
	left, right := net.Pipe()
	defer left.Close()
	defer right.Close()
	aConn := claimedConn{left, &net.TCPAddr{IP: net.ParseIP("9.9.9.9"), Port: 41000}, &net.TCPAddr{IP: net.ParseIP("8.8.8.8"), Port: 31000}}
	bConn := claimedConn{right, &net.TCPAddr{IP: net.ParseIP("8.8.8.8"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("9.9.9.9"), Port: 41000}}
	response := make(chan error, 1)
	go func() {
		conn, err := b.Upgrade(log.NewNopLogger())(bConn, nil, 5*time.Second)
		if conn != nil {
			_ = conn.Close()
		}
		response <- err
	}()
	conn, err := a.Upgrade(log.NewNopLogger())(aConn, &p2p.NetAddress{ID: keyB.ID(), IP: net.ParseIP("8.8.8.8"), Port: 31000}, 5*time.Second)
	if conn != nil {
		_ = conn.Close()
	}
	if responderErr := <-response; err != nil || responderErr != nil {
		t.Fatalf("pinned candidate handshake failed: %v / %v", err, responderErr)
	}
	wrongLeft, wrongRight := net.Pipe()
	defer wrongLeft.Close()
	defer wrongRight.Close()
	wrongSource := claimedConn{wrongLeft, &net.TCPAddr{IP: net.ParseIP("8.8.8.8"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("8.8.4.4"), Port: 41000}}
	if _, err := b.Upgrade(log.NewNopLogger())(wrongSource, nil, time.Second); err == nil {
		t.Fatal("unpinned candidate source IP accepted")
	}
}

func TestProductionCandidateChecksPinnedTransportWithoutStartup(t *testing.T) {
	for _, chainID := range []string{"mainnet", "dytallix-1", "e01-candidate-", "e01-candidate-mainnet"} {
		if candidateStagingChain(chainID) {
			t.Fatalf("nonstaging chain admitted for candidate startup: %s", chainID)
		}
	}
	if !candidateStagingChain("e01-candidate-peer-check") {
		t.Fatal("reserved candidate staging chain rejected")
	}
	runtime, local := productionCandidateFixture(t)
	if err := ValidateProductionTransportCandidate(runtime.Config, runtime.Transport, local, "proposed-chain"); err != nil {
		t.Fatal(err)
	}
	if _, err := Load(runtime.Config.RootDir, ProductionCandidateProfile); err == nil {
		t.Fatal("production candidate entered the engine loader")
	}
	for _, address := range []string{"127.0.0.1:31000", "0.0.0.0:31000", "localhost:31000", "[fe80::1]:31000", "10.1.2.2:80", "10.1.2.2:031000"} {
		if explicitPeerEndpoint(address) {
			t.Fatalf("unsupported endpoint accepted: %s", address)
		}
	}
	for _, address := range []string{"10.1.2.2:31000", "9.9.9.9:31000", "[fd00::2]:31000"} {
		if !explicitPeerEndpoint(address) {
			t.Fatalf("explicit endpoint rejected: %s", address)
		}
	}

	runtime.Config.P2P.AllowDuplicateIP = true
	if err := ValidateProductionTransportCandidate(runtime.Config, runtime.Transport, local, "proposed-chain"); err == nil {
		t.Fatal("duplicate-IP admission accepted")
	}
	runtime.Config.P2P.AllowDuplicateIP = false
	runtime.Config.RPC.CORSAllowedOrigins = []string{"http://127.0.0.1:4173"}
	if err := ValidateProductionTransportCandidate(runtime.Config, runtime.Transport, local, "proposed-chain"); err == nil {
		t.Fatal("browser origin accepted")
	}
	runtime.Config.RPC.CORSAllowedOrigins = nil
	runtime.Transport.Peers[0].Address = "10.1.2.3:31000"
	if err := ValidateProductionTransportCandidate(runtime.Config, runtime.Transport, local, "proposed-chain"); err == nil {
		t.Fatal("peer address differing from persistent pin accepted")
	}
}
