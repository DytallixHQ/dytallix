package enginepqc

import (
	"bytes"
	"encoding/base64"
	"io"
	"net"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func TestPrivateProfileAddressAndIsolation(t *testing.T) {
	for _, address := range []string{"10.1.2.3:31000", "172.16.0.1:31000", "192.168.1.2:31000", "[fd00::1]:31000"} {
		if !privateEndpoint(address) {
			t.Fatalf("private endpoint rejected: %s", address)
		}
	}
	for _, address := range []string{"127.0.0.1:31000", "0.0.0.0:31000", "203.0.113.4:31000", "localhost:31000", "10.1.2.3:80", "10.1.2.3:0", "[fe80::1]:31000", "[::]:31000"} {
		if privateEndpoint(address) {
			t.Fatalf("nonprivate endpoint accepted: %s", address)
		}
	}
	c := isolatedConfig(t)
	c.P2P.ListenAddress = "tcp://10.1.2.3:31000"
	c.P2P.AllowDuplicateIP = false
	c.P2P.AddrBookStrict = true
	if err := validateIsolationForProfile(c, RemoteSeedProfile); err != nil {
		t.Fatal(err)
	}
	if err := validateIsolationForProfile(c, SeedProfile); err == nil {
		t.Fatal("loopback profile accepted remote listener")
	}
	c.P2P.AllowDuplicateIP = true
	if err := validateIsolationForProfile(c, RemoteSeedProfile); err == nil {
		t.Fatal("remote profile accepted duplicate-IP admission")
	}
	c.P2P.AllowDuplicateIP = false
	c.RPC.ListenAddress = "tcp://10.1.2.3:31001"
	if err := validateIsolationForProfile(c, RemoteSeedProfile); err == nil {
		t.Fatal("remote profile accepted exposed RPC")
	}
}

func TestPrivateProfilePinsRequireDistinctPrivateHosts(t *testing.T) {
	local, remote, third := testKey(t), testKey(t), testKey(t)
	c := isolatedConfig(t)
	c.P2P.ListenAddress = "tcp://10.1.2.1:31000"
	c.P2P.PersistentPeers = string(remote.ID()) + "@10.1.2.2:31000"
	tc := TransportConfig{Version: 1, Profile: RemoteSeedProfile, Network: "private-fixture", LocalPublicKeyBase64: base64.StdEncoding.EncodeToString(local.PubKey().Bytes()), Peers: []PeerPin{{string(remote.ID()), base64.StdEncoding.EncodeToString(remote.PubKey().Bytes()), "10.1.2.2:31000"}}, HandshakeTimeoutMS: 5000}
	if _, _, _, err := validatePinsForProfile(tc, c, local, "private-fixture", RemoteSeedProfile); err != nil {
		t.Fatal(err)
	}
	for _, address := range []string{"127.0.0.1:31000", "203.0.113.4:31000", "10.1.2.1:31000", "10.1.2.2:80"} {
		copy := tc
		copy.Peers = []PeerPin{{string(remote.ID()), base64.StdEncoding.EncodeToString(remote.PubKey().Bytes()), address}}
		c.P2P.PersistentPeers = string(remote.ID()) + "@" + address
		if _, _, _, err := validatePinsForProfile(copy, c, local, "private-fixture", RemoteSeedProfile); err == nil {
			t.Fatalf("unapproved peer address accepted: %s", address)
		}
	}
	c.P2P.PersistentPeers = string(remote.ID()) + "@10.1.2.2:31000," + string(third.ID()) + "@10.1.2.2:31001"
	tc.Peers = append(tc.Peers, PeerPin{string(third.ID()), base64.StdEncoding.EncodeToString(third.PubKey().Bytes()), "10.1.2.2:31001"})
	if _, _, _, err := validatePinsForProfile(tc, c, local, "private-fixture", RemoteSeedProfile); err == nil {
		t.Fatal("two pinned peers on one private host accepted")
	}
}

type claimedConn struct {
	net.Conn
	local, remote net.Addr
}

func (c claimedConn) LocalAddr() net.Addr  { return c.local }
func (c claimedConn) RemoteAddr() net.Addr { return c.remote }

func testPrivateRuntime(t *testing.T, seedByte byte, listen, peer string, peerKey *p2p.NodeKey) *Runtime {
	t.Helper()
	seed := bytes.Repeat([]byte{seedByte}, mldsa65.SeedSize)
	private, err := mldsa65.GenPrivKeyFromSeed(seed)
	if err != nil {
		t.Fatal(err)
	}
	key := &p2p.NodeKey{PrivKey: private}
	identity, err := pqcp2p.ImportSeedIdentity(seed, key.PubKey().Bytes())
	clear(seed)
	if err != nil {
		t.Fatal(err)
	}
	c := isolatedConfig(t)
	c.P2P.ListenAddress = "tcp://" + listen
	c.P2P.AllowDuplicateIP = false
	c.P2P.AddrBookStrict = true
	return &Runtime{Config: c, NodeKey: key, Transport: TransportConfig{Profile: RemoteSeedProfile, Network: "private-fixture", HandshakeTimeoutMS: 5000}, identity: identity, pins: [][]byte{peerKey.PubKey().Bytes()}, byID: map[p2p.ID][]byte{peerKey.ID(): peerKey.PubKey().Bytes()}, addresses: map[p2p.ID]string{peerKey.ID(): peer}, slots: make(chan struct{}, MaxConcurrentHandshakes)}
}

func TestPrivateProfileAuthenticatedTransportSourceBinding(t *testing.T) {
	seedA := bytes.Repeat([]byte{1}, mldsa65.SeedSize)
	seedB := bytes.Repeat([]byte{2}, mldsa65.SeedSize)
	privateA, _ := mldsa65.GenPrivKeyFromSeed(seedA)
	privateB, _ := mldsa65.GenPrivKeyFromSeed(seedB)
	keyA := &p2p.NodeKey{PrivKey: privateA}
	keyB := &p2p.NodeKey{PrivKey: privateB}
	a := testPrivateRuntime(t, 1, "10.1.2.1:31000", "10.1.2.2:31000", keyB)
	b := testPrivateRuntime(t, 2, "10.1.2.2:31000", "10.1.2.1:31000", keyA)
	left, right := net.Pipe()
	defer left.Close()
	defer right.Close()
	aConn := claimedConn{left, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}}
	bConn := claimedConn{right, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}}
	type result struct {
		conn p2p.AuthenticatedConn
		err  error
	}
	response := make(chan result, 1)
	go func() {
		conn, err := b.Upgrade(log.NewNopLogger())(bConn, nil, 5*time.Second)
		response <- result{conn, err}
	}()
	initiated, err := a.Upgrade(log.NewNopLogger())(aConn, &p2p.NetAddress{ID: keyB.ID(), IP: net.ParseIP("10.1.2.2"), Port: 31000}, 5*time.Second)
	accepted := <-response
	if err != nil || accepted.err != nil {
		t.Fatalf("private authenticated transport: %v / %v", err, accepted.err)
	}
	defer initiated.Close()
	defer accepted.conn.Close()
	sent := make(chan error, 1)
	go func() { _, e := initiated.Write([]byte("private-record")); sent <- e }()
	buf := make([]byte, len("private-record"))
	if _, err := io.ReadFull(accepted.conn, buf); err != nil || string(buf) != "private-record" {
		t.Fatalf("private authenticated record failed: %v", err)
	}
	if err := <-sent; err != nil {
		t.Fatal(err)
	}
	badLeft, badRight := net.Pipe()
	defer badLeft.Close()
	defer badRight.Close()
	bad := claimedConn{badLeft, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.3"), Port: 41000}}
	if _, err := b.Upgrade(log.NewNopLogger())(bad, nil, time.Second); err == nil {
		t.Fatal("unpinned private source IP accepted")
	}
	wrongLeft, wrongRight := net.Pipe()
	defer wrongLeft.Close()
	defer wrongRight.Close()
	wrongOutbound := claimedConn{wrongLeft, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41001}, &net.TCPAddr{IP: net.ParseIP("10.1.2.3"), Port: 31000}}
	if _, err := a.Upgrade(log.NewNopLogger())(wrongOutbound, &p2p.NetAddress{ID: keyB.ID(), IP: net.ParseIP("10.1.2.2"), Port: 31000}, time.Second); err == nil {
		t.Fatal("outbound live endpoint differs from pin but was accepted")
	}
	thirdSeed := bytes.Repeat([]byte{3}, mldsa65.SeedSize)
	thirdPrivate, _ := mldsa65.GenPrivKeyFromSeed(thirdSeed)
	third := &p2p.NodeKey{PrivKey: thirdPrivate}
	b.byID[third.ID()] = third.PubKey().Bytes()
	b.addresses[third.ID()] = "10.1.2.3:31000"
	b.pins = append(b.pins, third.PubKey().Bytes())
	mismatchLeft, mismatchRight := net.Pipe()
	defer mismatchLeft.Close()
	defer mismatchRight.Close()
	mismatchA := claimedConn{mismatchLeft, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41002}, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}}
	mismatchB := claimedConn{mismatchRight, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.3"), Port: 41002}}
	mismatchResult := make(chan error, 1)
	go func() {
		conn, err := b.Upgrade(log.NewNopLogger())(mismatchB, nil, 5*time.Second)
		if conn != nil {
			_ = conn.Close()
		}
		mismatchResult <- err
	}()
	conn, err := a.Upgrade(log.NewNopLogger())(mismatchA, &p2p.NetAddress{ID: keyB.ID(), IP: net.ParseIP("10.1.2.2"), Port: 31000}, 5*time.Second)
	if conn != nil {
		_ = conn.Close()
	}
	responderErr := <-mismatchResult
	if responderErr == nil {
		t.Fatalf("pinned key accepted from another pinned host; initiator error: %v", err)
	}
}

func TestPrivateProfileRejectsWrongKeyFromPinnedAddress(t *testing.T) {
	keyForSeed := func(value byte) *p2p.NodeKey {
		private, err := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{value}, mldsa65.SeedSize))
		if err != nil {
			t.Fatal(err)
		}
		return &p2p.NodeKey{PrivKey: private}
	}
	aKey := keyForSeed(1)
	bKey := keyForSeed(2)
	a := testPrivateRuntime(t, 1, "10.1.2.1:31000", "10.1.2.2:31000", bKey)
	impostor := testPrivateRuntime(t, 3, "10.1.2.2:31000", "10.1.2.1:31000", aKey)
	left, right := net.Pipe()
	defer left.Close()
	defer right.Close()
	aConn := claimedConn{left, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}}
	iConn := claimedConn{right, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}}
	response := make(chan error, 1)
	go func() {
		conn, err := impostor.Upgrade(log.NewNopLogger())(iConn, nil, 3*time.Second)
		if conn != nil {
			_ = conn.Close()
		}
		response <- err
	}()
	conn, err := a.Upgrade(log.NewNopLogger())(aConn, &p2p.NetAddress{ID: bKey.ID(), IP: net.ParseIP("10.1.2.2"), Port: 31000}, 3*time.Second)
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("wrong ML-DSA key accepted from pinned private address")
	}
	_ = right.Close()
	<-response
}
