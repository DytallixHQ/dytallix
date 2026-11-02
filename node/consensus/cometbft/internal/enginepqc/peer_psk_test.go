package enginepqc

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func writePSKFile(t *testing.T, home string, mode os.FileMode, keys map[string]string) {
	t.Helper()
	raw, err := json.Marshal(pskFile{Version: 1, Keys: keys})
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, "config", PSKFileName)
	_ = os.Remove(path)
	if err = os.WriteFile(path, raw, mode); err != nil {
		t.Fatal(err)
	}
	if err = os.Chmod(path, mode); err != nil {
		t.Fatal(err)
	}
}

func TestPeerPSKFileIsOptionalPrivateAndBoundToPins(t *testing.T) {
	home := t.TempDir()
	if err := os.Mkdir(filepath.Join(home, "config"), 0o700); err != nil {
		t.Fatal(err)
	}
	peer, _ := mldsa65.GenPrivKey()
	pin := peer.PubKey().Bytes()
	stranger, _ := mldsa65.GenPrivKey()
	key := base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{5}, 32))

	if keys, err := loadPeerPSKs(home, [][]byte{pin}); err != nil || len(keys) != 0 {
		t.Fatalf("missing file: %v %v", keys, err)
	}
	writePSKFile(t, home, 0o600, map[string]string{base64.StdEncoding.EncodeToString(pin): key})
	keys, err := loadPeerPSKs(home, [][]byte{pin})
	if err != nil || keys[string(pin)] != (pqcp2p.PSK{5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5}) {
		t.Fatalf("valid file: %v", err)
	}
	for name, c := range map[string]struct {
		mode os.FileMode
		keys map[string]string
	}{
		"group readable": {0o640, map[string]string{base64.StdEncoding.EncodeToString(pin): key}},
		"unpinned peer":  {0o600, map[string]string{base64.StdEncoding.EncodeToString(stranger.PubKey().Bytes()): key}},
		"zero key":       {0o600, map[string]string{base64.StdEncoding.EncodeToString(pin): base64.StdEncoding.EncodeToString(make([]byte, 32))}},
		"short key":      {0o600, map[string]string{base64.StdEncoding.EncodeToString(pin): base64.StdEncoding.EncodeToString(make([]byte, 31))}},
	} {
		writePSKFile(t, home, c.mode, c.keys)
		if _, err := loadPeerPSKs(home, [][]byte{pin}); err == nil {
			t.Fatalf("%s accepted", name)
		}
	}
}

// The engine's own Upgrade path: matching PSKs connect, a peer holding the
// right ML-DSA identity but the wrong PSK is refused.
func TestEngineUpgradeEnforcesPeerPSK(t *testing.T) {
	seedA := bytes.Repeat([]byte{1}, mldsa65.SeedSize)
	seedB := bytes.Repeat([]byte{2}, mldsa65.SeedSize)
	privateA, _ := mldsa65.GenPrivKeyFromSeed(seedA)
	privateB, _ := mldsa65.GenPrivKeyFromSeed(seedB)
	keyA := &p2p.NodeKey{PrivKey: privateA}
	keyB := &p2p.NodeKey{PrivKey: privateB}
	for _, c := range []struct {
		name   string
		a, b   pqcp2p.PSK
		accept bool
	}{
		{"matching", pqcp2p.PSK{3}, pqcp2p.PSK{3}, true},
		{"mismatched", pqcp2p.PSK{3}, pqcp2p.PSK{4}, false},
	} {
		a := testPrivateRuntime(t, 1, "10.1.2.1:31000", "10.1.2.2:31000", keyB)
		b := testPrivateRuntime(t, 2, "10.1.2.2:31000", "10.1.2.1:31000", keyA)
		a.psk = map[string]pqcp2p.PSK{string(keyB.PubKey().Bytes()): c.a}
		b.psk = map[string]pqcp2p.PSK{string(keyA.PubKey().Bytes()): c.b}
		left, right := net.Pipe()
		aConn := claimedConn{left, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}}
		bConn := claimedConn{right, &net.TCPAddr{IP: net.ParseIP("10.1.2.2"), Port: 31000}, &net.TCPAddr{IP: net.ParseIP("10.1.2.1"), Port: 41000}}
		errs := make(chan error, 1)
		go func() {
			conn, err := b.Upgrade(log.NewNopLogger())(bConn, nil, 5*time.Second)
			if conn != nil {
				defer conn.Close()
			}
			errs <- err
		}()
		conn, err := a.Upgrade(log.NewNopLogger())(aConn, &p2p.NetAddress{ID: keyB.ID(), IP: net.ParseIP("10.1.2.2"), Port: 31000}, 5*time.Second)
		if conn != nil {
			conn.Close()
		}
		other := <-errs
		if c.accept != (err == nil && other == nil) {
			t.Fatalf("%s: initiator %v, responder %v", c.name, err, other)
		}
		left.Close()
		right.Close()
	}
}
