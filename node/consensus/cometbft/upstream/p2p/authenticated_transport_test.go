package p2p

import (
	"errors"
	"net"
	"testing"
	"time"

	"github.com/cometbft/cometbft/p2p/conn"
)

func TestExplicitAuthenticatedUpgradeRejectsWithoutLegacyFallback(t *testing.T) {
	transport := NewMultiplexTransport(DefaultNodeInfo{}, NodeKey{}, conn.MConnConfig{})
	called := 0
	err := transport.SetAuthenticatedConnUpgrade(func(raw net.Conn, dialed *NetAddress, timeout time.Duration) (AuthenticatedConn, error) {
		called++
		return nil, errors.New("explicit profile rejection")
	})
	if err != nil {
		t.Fatal(err)
	}
	if transport.SetAuthenticatedConnUpgrade(nil) == nil {
		t.Fatal("upgrade could be cleared")
	}
	for _, dialed := range []*NetAddress{nil, {ID: "fixture-outbound"}} {
		left, right := net.Pipe()
		_, _, err := transport.upgrade(left, dialed)
		right.Close()
		if err == nil {
			t.Fatal("callback rejection accepted")
		}
	}
	// Empty NodeKey would panic if the legacy key exchange were attempted.
	if called != 2 {
		t.Fatalf("both directions must invoke the callback; got %d", called)
	}
}
