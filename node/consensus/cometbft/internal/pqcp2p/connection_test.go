package pqcp2p

import (
	"bytes"
	"errors"
	"io"
	"net"
	"testing"
	"time"

	"golang.org/x/crypto/chacha20poly1305"
)

type shortConn struct{ net.Conn }

func (c shortConn) Read(p []byte) (int, error)  { return c.Conn.Read(p[:min(7, len(p))]) }
func (c shortConn) Write(p []byte) (int, error) { return c.Conn.Write(p[:min(11, len(p))]) }

type upgraded struct {
	conn *Conn
	err  error
}

func upgradePair(t *testing.T, a, b *Identity, loopback, short bool) (*Conn, *Conn) {
	t.Helper()
	var left, right net.Conn
	if loopback {
		listener, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		defer listener.Close()
		ready := make(chan net.Conn, 1)
		go func() { c, _ := listener.Accept(); ready <- c }()
		left, err = net.DialTimeout("tcp", listener.Addr().String(), time.Second)
		if err != nil {
			t.Fatal(err)
		}
		right = <-ready
		if right == nil {
			t.Fatal("accept failed")
		}
	} else {
		left, right = net.Pipe()
	}
	if short {
		left = shortConn{left}
		right = shortConn{right}
	}
	result := make(chan upgraded, 1)
	go func() {
		c, e := Upgrade(right, "fixture-network", b, [][]byte{a.PublicKey()}, nil, 3*time.Second)
		result <- upgraded{c, e}
	}()
	c, e := Upgrade(left, "fixture-network", a, [][]byte{b.PublicKey()}, b.PublicKey(), 3*time.Second)
	r := <-result
	if e != nil || r.err != nil {
		if c != nil {
			c.Close()
		}
		if r.conn != nil {
			r.conn.Close()
		}
		t.Fatalf("upgrade: %v / %v", e, r.err)
	}
	t.Cleanup(func() { c.Close(); r.conn.Close() })
	return c, r.conn
}

func TestConnectionLoopbackRoundTrip(t *testing.T) {
	a, b := identity(t), identity(t)
	c, d := upgradePair(t, a, b, true, false)
	if !bytes.Equal(c.PeerPublicKey(), b.PublicKey()) || !bytes.Equal(d.PeerPublicKey(), a.PublicKey()) {
		t.Fatal("peer identity")
	}
	exposed := c.PeerPublicKey()
	exposed[0] ^= 1
	if bytes.Equal(exposed, c.PeerPublicKey()) {
		t.Fatal("peer identity aliases")
	}
	message := bytes.Repeat([]byte("bounded-data"), 5000)
	sent := make(chan error, 1)
	go func() {
		n, e := c.Write(message)
		if e == nil && n != len(message) {
			e = io.ErrShortWrite
		}
		sent <- e
	}()
	received := make([]byte, len(message))
	if _, e := io.ReadFull(d, received); e != nil {
		t.Fatal(e)
	}
	if e := <-sent; e != nil {
		t.Fatal(e)
	}
	if !bytes.Equal(received, message) {
		t.Fatal("plaintext mismatch")
	}
	go func() { _, e := d.Write([]byte("reply")); sent <- e }()
	var reply [5]byte
	if _, e := io.ReadFull(c, reply[:]); e != nil {
		t.Fatal(e)
	}
	if e := <-sent; e != nil {
		t.Fatal(e)
	}
	if string(reply[:]) != "reply" {
		t.Fatal("reply mismatch")
	}
}
func TestConnectionPartialIOAndFreshSession(t *testing.T) {
	a, b := identity(t), identity(t)
	c, d := upgradePair(t, a, b, false, true)
	ciphertext := c.send.Seal(nil, make([]byte, 12), []byte("same"), nil)
	c.Close()
	d.Close()
	e, f := upgradePair(t, a, b, false, true)
	if bytes.Equal(ciphertext, e.send.Seal(nil, make([]byte, 12), []byte("same"), nil)) {
		t.Fatal("session keys reused")
	}
	done := make(chan error, 1)
	go func() { _, err := e.Write(bytes.Repeat([]byte{42}, 100)); done <- err }()
	result := make([]byte, 100)
	if _, err := io.ReadFull(f, result); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(result, bytes.Repeat([]byte{42}, 100)) {
		t.Fatal("short I/O corrupted data")
	}
}
func TestConnectionWrongNetworkAndStrictPins(t *testing.T) {
	a, b := identity(t), identity(t)
	for _, network := range []string{"wrong-network", "fixture-network"} {
		left, right := net.Pipe()
		result := make(chan error, 1)
		pins := [][]byte{a.PublicKey()}
		if network == "fixture-network" {
			pins = [][]byte{identity(t).PublicKey()}
		}
		go func() { _, err := Upgrade(right, network, b, pins, nil, time.Second); result <- err }()
		c, err := Upgrade(left, "fixture-network", a, [][]byte{b.PublicKey()}, b.PublicKey(), time.Second)
		if err == nil || c != nil {
			t.Fatal("unexpected authorization")
		}
		if err := <-result; err == nil {
			t.Fatal("responder authorized")
		}
	}
	for _, pins := range [][][]byte{nil, {a.PublicKey()}, {b.PublicKey(), b.PublicKey()}, {make([]byte, 32)}} {
		left, right := net.Pipe()
		_, err := Upgrade(left, "fixture", a, pins, b.PublicKey(), time.Second)
		right.Close()
		if err == nil {
			t.Fatal("bad pins accepted")
		}
	}
}
func TestConnectionHandshakeDeadlineAndHeaderBounds(t *testing.T) {
	a, b := identity(t), identity(t)
	left, right := net.Pipe()
	defer right.Close()
	start := time.Now()
	_, err := Upgrade(left, "fixture", b, [][]byte{a.PublicKey()}, nil, 25*time.Millisecond)
	if err == nil || time.Since(start) > time.Second {
		t.Fatal("handshake deadline did not bound I/O")
	}
	for _, h := range [][]byte{{'D', 'Y', 'P', 'H', 1, helloType, 255, 255}, {'D', 'Y', 'P', 'H', 2, helloType, 15, 98}, {'D', 'Y', 'P', 'H', 1, offerType, 0, 32}} {
		if _, err := readHandshake(bytes.NewReader(h), helloType); !errors.Is(err, ErrRejected) {
			t.Fatalf("header accepted: %v", err)
		}
	}
}

func recordPair(t *testing.T) (*Conn, net.Conn) {
	t.Helper()
	raw, peer := net.Pipe()
	key := make([]byte, 32)
	key[0] = 1
	aead, err := chacha20poly1305.New(key)
	if err != nil {
		t.Fatal(err)
	}
	c := &Conn{raw: raw, send: aead, receive: aead}
	t.Cleanup(func() { c.Close(); peer.Close() })
	return c, peer
}
func frame(c *Conn, seq uint64, p []byte) []byte {
	h := recordHeader(seq, len(p))
	nonce := recordNonce(seq)
	return append(h[:], c.receive.Seal(nil, nonce[:], p, h[:])...)
}
func TestConnectionRejectsCorruptSequenceLengthAndPartialRecords(t *testing.T) {
	for _, kind := range []string{"tag", "sequence", "length", "partial"} {
		t.Run(kind, func(t *testing.T) {
			c, peer := recordPair(t)
			record := frame(c, 0, []byte("authenticated"))
			switch kind {
			case "tag":
				record[len(record)-1] ^= 1
			case "sequence":
				record[13] = 1
			case "length":
				record[14] = 255
				record[15] = 255
			case "partial":
				record = record[:len(record)-1]
			}
			go func() { _, _ = peer.Write(record); peer.Close() }()
			buf := make([]byte, 40)
			n, err := c.Read(buf)
			if n != 0 || err == nil {
				t.Fatal("corrupt record exposed plaintext")
			}
			if n, err = c.Write([]byte("x")); n != 0 || !errors.Is(err, net.ErrClosed) {
				t.Fatal("failed stream reopened")
			}
		})
	}
}
func TestConnectionCloseDiscardsCachedPlaintext(t *testing.T) {
	c, peer := recordPair(t)
	go func() { _, _ = peer.Write(frame(c, 0, []byte("secret"))) }()
	var first [1]byte
	if _, err := c.Read(first[:]); err != nil {
		t.Fatal(err)
	}
	if len(c.pending) == 0 {
		t.Fatal("test has no pending bytes")
	}
	c.Close()
	if n, err := c.Read(make([]byte, 20)); n != 0 || !errors.Is(err, net.ErrClosed) {
		t.Fatal("plaintext returned after close")
	}
	if len(c.pending) != 0 {
		t.Fatal("pending bytes not cleared")
	}
}
func TestConnectionRecordCeilingAndReplay(t *testing.T) {
	c, peer := recordPair(t)
	c.writeSequence = MaxRecords
	if n, err := c.Write([]byte("x")); n != 0 || !errors.Is(err, ErrRecordLimit) {
		t.Fatal("send ceiling")
	}
	peer.Close()
	d, other := recordPair(t)
	d.readSequence = MaxRecords
	if n, err := d.Read(make([]byte, 1)); n != 0 || !errors.Is(err, ErrRecordLimit) {
		t.Fatal("receive ceiling")
	}
	other.Close()
	e, remote := recordPair(t)
	record := frame(e, 0, []byte("x"))
	go func() { _, _ = remote.Write(record); _, _ = remote.Write(record); remote.Close() }()
	var buf [1]byte
	if _, err := e.Read(buf[:]); err != nil {
		t.Fatal(err)
	}
	if n, err := e.Read(buf[:]); n != 0 || !errors.Is(err, ErrRecord) {
		t.Fatal("replay accepted")
	}
}
func TestImportIdentityEncodingAndSigningCapability(t *testing.T) {
	original := identity(t)
	packed := original.private.Bytes()
	defer clear(packed)
	imported, err := ImportIdentity(packed)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(imported.PublicKey(), original.PublicKey()) {
		t.Fatal("import changed identity")
	}
	for _, offset := range []int{64, 128} {
		broken := bytes.Clone(packed)
		if offset == 128 {
			broken[offset] = 255
		} else {
			broken[offset] ^= 1
		}
		_, err := ImportIdentity(broken)
		clear(broken)
		if err == nil {
			t.Fatalf("inconsistent private field accepted at %d", offset)
		}
	}
	// A small t0 change can still pass the signing-capability check. The
	// experimental importer does not promise to reject it. If accepted, the
	// public identity remains the same; this is not a consistency certificate.
	changedT0 := bytes.Clone(packed)
	changedT0[1536] ^= 1
	limited, limitedErr := ImportIdentity(changedT0)
	clear(changedT0)
	if limitedErr == nil {
		if limited == nil || !bytes.Equal(limited.PublicKey(), original.PublicKey()) {
			t.Fatal("t0 changed the derived public identity")
		}
	} else if !errors.Is(limitedErr, ErrRejected) {
		t.Fatal(limitedErr)
	}
	if _, err := ImportIdentity(packed[:32]); err == nil {
		t.Fatal("seed accepted as packed key")
	}
	if !errors.Is(RequireProductionTransport(), ErrProductionBlocked) {
		t.Fatal("production gate opened")
	}
}
