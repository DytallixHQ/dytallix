// dytallix-pqc-peer-probe performs one staging-only, authenticated TCP exchange.
// It does not start consensus, create keys, or authorize production operation.
package main

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"strings"
	"time"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

const challengeSize = 32
const maxProbeDuration = 30 * time.Second

type probeOptions struct {
	home, role, peerID string
	duration           time.Duration
}

func parseOptions(args []string) (probeOptions, error) {
	flags := flag.NewFlagSet("dytallix-pqc-peer-probe", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	home := flags.String("home", "", "existing nonproduction fixture home")
	role := flags.String("role", "", "listen or dial")
	peerID := flags.String("peer-id", "", "exact configured peer ID")
	duration := flags.Duration("timeout", 20*time.Second, "total probe timeout")
	production := flags.Bool("production", false, "always refused")
	if err := flags.Parse(args); err != nil {
		return probeOptions{}, err
	}
	if *production {
		return probeOptions{}, errors.New("production peer probe is prohibited")
	}
	if flags.NArg() != 0 || *home == "" || (*role != "listen" && *role != "dial") || *peerID == "" || *duration < time.Second || *duration > maxProbeDuration {
		return probeOptions{}, errors.New("require --home, --role listen|dial, --peer-id, and timeout from 1s to 30s")
	}
	return probeOptions{*home, *role, *peerID, *duration}, nil
}

func peerAddress(r *enginepqc.Runtime, id string) (string, p2p.ID, error) {
	for _, peer := range r.Transport.Peers {
		if peer.ID == id {
			return peer.Address, p2p.ID(id), nil
		}
	}
	return "", "", errors.New("selected peer is absent from the full-key transport pins")
}

func readFrame(conn net.Conn, size int, kind byte) ([]byte, error) {
	frame := make([]byte, size)
	if _, err := io.ReadFull(conn, frame); err != nil {
		return nil, err
	}
	if frame[0] != kind {
		return nil, errors.New("probe frame type mismatch")
	}
	return frame, nil
}

func writeFrame(conn net.Conn, frame []byte) error {
	n, err := conn.Write(frame)
	if err != nil {
		return err
	}
	if n != len(frame) {
		return io.ErrShortWrite
	}
	return nil
}

// The transport has already authenticated the peer and encrypts every frame.
// Both random challenges must return before either role records a pass.
// The digest binds the two host reports to one completed exchange.
func exchange(conn net.Conn, role string) (string, error) {
	challenge := make([]byte, challengeSize)
	if _, err := rand.Read(challenge); err != nil {
		return "", err
	}
	if role == "dial" {
		if err := writeFrame(conn, append([]byte{1}, challenge...)); err != nil {
			return "", err
		}
		response, err := readFrame(conn, 1+2*challengeSize, 2)
		if err != nil {
			return "", err
		}
		if !bytes.Equal(response[1:1+challengeSize], challenge) {
			return "", errors.New("dial challenge mismatch")
		}
		if err := writeFrame(conn, append([]byte{3}, response[1+challengeSize:]...)); err != nil {
			return "", err
		}
		ack, err := readFrame(conn, 1+challengeSize, 4)
		if err != nil {
			return "", err
		}
		if !bytes.Equal(ack[1:], challenge) {
			return "", errors.New("final acknowledgement mismatch")
		}
		return sessionDigest(challenge, response[1+challengeSize:]), nil
	}
	request, err := readFrame(conn, 1+challengeSize, 1)
	if err != nil {
		return "", err
	}
	response := make([]byte, 1+2*challengeSize)
	response[0] = 2
	copy(response[1:], request[1:])
	copy(response[1+challengeSize:], challenge)
	if err := writeFrame(conn, response); err != nil {
		return "", err
	}
	confirmation, err := readFrame(conn, 1+challengeSize, 3)
	if err != nil {
		return "", err
	}
	if !bytes.Equal(confirmation[1:], challenge) {
		return "", errors.New("listener challenge mismatch")
	}
	if err := writeFrame(conn, append([]byte{4}, request[1:]...)); err != nil {
		return "", err
	}
	return sessionDigest(request[1:], challenge), nil
}

func sessionDigest(dialChallenge, listenChallenge []byte) string {
	var transcript [2 * challengeSize]byte
	copy(transcript[:challengeSize], dialChallenge)
	copy(transcript[challengeSize:], listenChallenge)
	sum := sha256.Sum256(transcript[:])
	return hex.EncodeToString(sum[:])
}

func run(args []string, output io.Writer) error {
	options, err := parseOptions(args)
	if err != nil {
		return err
	}
	if enginepqc.BuildProfile != "dytallix_pqc_only" || enginepqc.RPCBuildProfile != "dytallix-pqc-unix-v1" {
		return errors.New("strict PQC-only IPC build tags are required")
	}
	runtime, err := enginepqc.Load(options.home, enginepqc.RemoteSeedProfile)
	if err != nil {
		return err
	}
	remote, selectedID, err := peerAddress(runtime, options.peerID)
	if err != nil {
		return err
	}
	local := strings.TrimPrefix(runtime.Config.P2P.ListenAddress, "tcp://")
	localHost, _, err := net.SplitHostPort(local)
	if err != nil {
		return err
	}
	localIP := net.ParseIP(localHost)
	if localIP == nil {
		return errors.New("configured listener has no literal private IP")
	}
	deadline := time.Now().Add(options.duration)
	var raw net.Conn
	if options.role == "listen" {
		listener, err := net.Listen("tcp", local)
		if err != nil {
			return err
		}
		defer listener.Close()
		tcpListener, ok := listener.(*net.TCPListener)
		if !ok {
			return errors.New("probe requires a TCP listener")
		}
		if err := tcpListener.SetDeadline(deadline); err != nil {
			return err
		}
		raw, err = tcpListener.Accept()
		if err != nil {
			return err
		}
		_ = listener.Close() // Refuse every connection after the first admission.
	} else {
		_, portText, err := net.SplitHostPort(remote)
		if err != nil {
			return err
		}
		port, err := strconv.Atoi(portText)
		if err != nil || port < 1024 || port > 65535 {
			return errors.New("invalid pinned peer port")
		}
		dialer := net.Dialer{LocalAddr: &net.TCPAddr{IP: localIP}, Timeout: time.Until(deadline)}
		raw, err = dialer.Dial("tcp", remote)
		if err != nil {
			return err
		}
	}
	defer raw.Close()
	// Runtime.Upgrade changes socket deadlines. Closing the raw socket keeps the
	// total operation bounded even if a peer stalls during or after admission.
	timer := time.AfterFunc(time.Until(deadline), func() { _ = raw.Close() })
	defer timer.Stop()
	logger := log.NewNopLogger()
	var dialed *p2p.NetAddress
	if options.role == "dial" {
		peerIPText, portText, _ := net.SplitHostPort(remote)
		port, _ := strconv.Atoi(portText)
		dialed = &p2p.NetAddress{ID: selectedID, IP: net.ParseIP(peerIPText), Port: uint16(port)}
	}
	conn, err := runtime.Upgrade(logger)(raw, dialed, time.Until(deadline))
	if err != nil {
		return err
	}
	defer conn.Close()
	if p2p.PubKeyToID(conn.RemotePubKey()) != selectedID {
		return errors.New("authenticated peer differs from selected pin")
	}
	if err := conn.SetDeadline(deadline); err != nil {
		return err
	}
	session, err := exchange(conn, options.role)
	if err != nil {
		return err
	}
	return json.NewEncoder(output).Encode(map[string]any{
		"result":               "PASS",
		"scope":                "staging_tcp_peer_probe",
		"profile":              enginepqc.RemoteSeedProfile,
		"network":              runtime.Transport.Network,
		"role":                 options.role,
		"local_id":             runtime.NodeKey.ID(),
		"peer_id":              selectedID,
		"session_sha256":       session,
		"production_qualified": false,
	})
}

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "peer probe failed:", err)
		os.Exit(1)
	}
}
