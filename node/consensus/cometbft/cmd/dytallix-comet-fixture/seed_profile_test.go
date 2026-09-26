package main

import (
	"io"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func TestSeedProfileFixtureLoadsWithoutPackedPeerKey(t *testing.T) {
	root, err := os.MkdirTemp("/tmp", "pqc-seed-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	app := filepath.Join(root, "app.json")
	if err := os.WriteFile(app, []byte(`{"chain_id":"seed-fixture"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(root, "nodes")
	nodes, err := generateWithTransport(output, app, "seed-fixture", "2026-01-01T00:00:00Z", 28650, "", false, pqcSeedLoopbackTransport)
	if err != nil {
		t.Fatal(err)
	}
	runtimes := make([]*enginepqc.Runtime, 0, len(nodes))
	for _, node := range nodes {
		if _, err := os.Lstat(filepath.Join(node.Home, "config", "node_key.json")); !os.IsNotExist(err) {
			t.Fatalf("packed peer key exists: %v", err)
		}
		first, err := enginepqc.Load(node.Home, enginepqc.SeedProfile)
		if err != nil {
			t.Fatal(err)
		}
		second, err := enginepqc.Load(node.Home, enginepqc.SeedProfile)
		if err != nil || second.NodeKey.ID() != first.NodeKey.ID() {
			t.Fatalf("restart changed identity: %v", err)
		}
		if first.PublicSummary()["profile"] != enginepqc.SeedProfile {
			t.Fatal("runtime reported wrong profile")
		}
		runtimes = append(runtimes, first)
		if _, err := enginepqc.Load(node.Home, enginepqc.Profile); err == nil {
			t.Fatal("packed-key profile accepted seed fixture")
		}
	}
	checkSeedHandshake(t, runtimes[0], runtimes[1])
	restarted, err := enginepqc.Load(nodes[0].Home, enginepqc.SeedProfile)
	if err != nil {
		t.Fatal(err)
	}
	checkSeedHandshake(t, restarted, runtimes[1])
}

func checkSeedHandshake(t *testing.T, initiator, responder *enginepqc.Runtime) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	type result struct {
		conn p2p.AuthenticatedConn
		err  error
	}
	remote := make(chan result, 1)
	go func() {
		raw, err := listener.Accept()
		if err != nil {
			remote <- result{err: err}
			return
		}
		conn, err := responder.Upgrade(log.NewNopLogger())(raw, nil, 5*time.Second)
		remote <- result{conn, err}
	}()
	raw, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	address := &p2p.NetAddress{ID: responder.NodeKey.ID(), IP: net.ParseIP("127.0.0.1"), Port: 28660}
	local, err := initiator.Upgrade(log.NewNopLogger())(raw, address, 5*time.Second)
	other := <-remote
	if err != nil || other.err != nil {
		t.Fatalf("seed-backed engine handshake: %v / %v", err, other.err)
	}
	defer local.Close()
	defer other.conn.Close()
	if local.RemotePubKey().Type() != responder.NodeKey.PubKey().Type() || local.RemotePubKey().Equals(responder.NodeKey.PubKey()) == false || other.conn.RemotePubKey().Equals(initiator.NodeKey.PubKey()) == false {
		t.Fatal("authenticated peer key differs from fixture pin")
	}
	_ = local.SetDeadline(time.Now().Add(3 * time.Second))
	_ = other.conn.SetDeadline(time.Now().Add(3 * time.Second))
	sent := make(chan error, 1)
	go func() { _, err := local.Write([]byte("seed-backed-round-trip")); sent <- err }()
	read := make([]byte, len("seed-backed-round-trip"))
	if _, err := io.ReadFull(other.conn, read); err != nil || string(read) != "seed-backed-round-trip" {
		t.Fatalf("authenticated record failed: %v", err)
	}
	if err := <-sent; err != nil {
		t.Fatal(err)
	}
}
