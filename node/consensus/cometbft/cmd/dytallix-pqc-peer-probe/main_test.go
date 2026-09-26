package main

import (
	"net"
	"strings"
	"testing"
	"time"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
)

func TestProbeRejectsProductionAndInvalidBounds(t *testing.T) {
	for _, args := range [][]string{
		{"--production", "--home", "/tmp/unused", "--role", "dial", "--peer-id", "peer"},
		{"--home", "/tmp/unused", "--role", "other", "--peer-id", "peer"},
		{"--home", "/tmp/unused", "--role", "dial", "--peer-id", "peer", "--timeout", "31s"},
		{"--home", "/tmp/unused", "--role", "dial", "--timeout", "1s"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("unsafe arguments accepted: %v", args)
		}
	}
	if _, err := parseOptions([]string{"--home", "/tmp/unused", "--role", "listen", "--peer-id", "peer", "--timeout", "1s"}); err != nil {
		t.Fatal(err)
	}
}

// This test uses real TCP. It proves both probe frames pass over authenticated
// PQC records, rather than only testing an in-memory connection pair.
func TestProbeChallengeOverAuthenticatedTCP(t *testing.T) {
	a, err := pqcp2p.GenerateIdentity()
	if err != nil {
		t.Fatal(err)
	}
	b, err := pqcp2p.GenerateIdentity()
	if err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	type result struct {
		session string
		err     error
	}
	serverResult := make(chan result, 1)
	go func() {
		raw, err := listener.Accept()
		if err != nil {
			serverResult <- result{err: err}
			return
		}
		defer raw.Close()
		conn, err := pqcp2p.Upgrade(raw, "probe-test", b, [][]byte{a.PublicKey()}, nil, 3*time.Second)
		var session string
		if err == nil {
			defer conn.Close()
			session, err = exchange(conn, "listen")
		}
		serverResult <- result{session, err}
	}()
	raw, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer raw.Close()
	conn, err := pqcp2p.Upgrade(raw, "probe-test", a, [][]byte{b.PublicKey()}, b.PublicKey(), 3*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if err := conn.SetDeadline(time.Now().Add(3 * time.Second)); err != nil {
		t.Fatal(err)
	}
	session, err := exchange(conn, "dial")
	if err != nil {
		t.Fatal(err)
	}
	server := <-serverResult
	if server.err != nil {
		t.Fatal(server.err)
	}
	if len(session) != 64 || session != server.session {
		t.Fatalf("authenticated hosts reported different sessions: %q / %q", session, server.session)
	}
}

func TestProbeRejectsBadChallengeResponse(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	serverResult := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			serverResult <- err
			return
		}
		defer conn.Close()
		request, err := readFrame(conn, 1+challengeSize, 1)
		if err != nil {
			serverResult <- err
			return
		}
		response := make([]byte, 1+2*challengeSize)
		response[0] = 2
		copy(response[1:], request[1:])
		response[1] ^= 1
		serverResult <- writeFrame(conn, response)
	}()
	conn, err := net.DialTimeout("tcp", listener.Addr().String(), time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(time.Second))
	if session, err := exchange(conn, "dial"); session != "" || err == nil || !strings.Contains(err.Error(), "dial challenge mismatch") {
		t.Fatalf("bad response accepted: %v", err)
	}
	if err := <-serverResult; err != nil {
		t.Fatal(err)
	}
}
