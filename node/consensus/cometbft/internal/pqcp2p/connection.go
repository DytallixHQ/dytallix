package pqcp2p

import (
	"bytes"
	"crypto/cipher"
	"crypto/mlkem"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
	"golang.org/x/crypto/chacha20poly1305"
)

const (
	WireVersion  = 1
	MaxPins      = 64
	MaxPlaintext = 16 * 1024
	// A direction can carry at most 16 GiB per session. Reconnect with fresh keys.
	MaxRecords          uint64 = 1 << 20
	MaxHandshakeTimeout        = time.Minute
	handshakeHeaderSize        = 8
	recordHeaderSize           = 16
	helloType           byte   = 1
	offerType           byte   = 2
	responseType        byte   = 3
	serverFinishedType  byte   = 4
	clientFinishedType  byte   = 5
	helloFixedSize             = 1 + 32 + 2*mldsa65.PublicKeySize
)

var ErrRecord = errors.New("PQC authenticated record rejected")
var ErrRecordLimit = errors.New("PQC session record limit reached; reconnect required")

// Conn implements net.Conn without exposing a plaintext bypass to its stream.
// A caller can use one reader and one writer concurrently. All failures are
// terminal. Close interrupts blocked I/O without waiting for either I/O lock.
type Conn struct {
	terminal                    atomic.Bool
	raw                         net.Conn
	peer                        []byte
	send, receive               cipher.AEAD
	readMu, writeMu             sync.Mutex
	readSequence, writeSequence uint64
	pending                     []byte
	closeOnce                   sync.Once
	closeError                  error
}

var _ net.Conn = (*Conn)(nil)

func (c *Conn) PeerPublicKey() []byte              { return bytes.Clone(c.peer) }
func (c *Conn) LocalAddr() net.Addr                { return c.raw.LocalAddr() }
func (c *Conn) RemoteAddr() net.Addr               { return c.raw.RemoteAddr() }
func (c *Conn) SetDeadline(t time.Time) error      { return c.raw.SetDeadline(t) }
func (c *Conn) SetReadDeadline(t time.Time) error  { return c.raw.SetReadDeadline(t) }
func (c *Conn) SetWriteDeadline(t time.Time) error { return c.raw.SetWriteDeadline(t) }
func (c *Conn) Close() error {
	c.terminal.Store(true)
	c.closeOnce.Do(func() { c.closeError = c.raw.Close() })
	return c.closeError
}
func (c *Conn) fail(err error) error { _ = c.Close(); return err }

// Upgrade consumes raw, including on failure. remotePin != nil selects the
// initiator role and must equal one configured full pin. A nil remotePin selects
// the responder role. There is no protocol negotiation or plaintext fallback.
// Callers must separately bound the number of concurrent handshake attempts.
func Upgrade(raw net.Conn, network string, local *Identity, pins [][]byte, remotePin []byte, timeout time.Duration) (result *Conn, err error) {
	if raw == nil {
		return nil, ErrRejected
	}
	defer func() {
		if result == nil {
			_ = raw.Close()
		}
	}()
	if len(network) == 0 || len(network) > 64 || local == nil || local.private == nil || local.public == nil || timeout <= 0 || timeout > MaxHandshakeTimeout {
		return nil, ErrRejected
	}
	if len(pins) == 0 || len(pins) > MaxPins {
		return nil, ErrRejected
	}
	own := local.PublicKey()
	allowed := make(map[string][]byte, len(pins))
	for _, pin := range pins {
		if len(pin) != mldsa65.PublicKeySize || bytes.Equal(pin, own) {
			return nil, ErrRejected
		}
		if _, duplicate := allowed[string(pin)]; duplicate {
			return nil, ErrRejected
		}
		allowed[string(pin)] = bytes.Clone(pin)
	}
	if remotePin != nil {
		if _, found := allowed[string(remotePin)]; !found {
			return nil, ErrRejected
		}
	}
	if err = raw.SetDeadline(time.Now().Add(timeout)); err != nil {
		return nil, err
	}
	var keys SessionKeys
	var peer []byte
	if remotePin != nil {
		keys, peer, err = upgradeInitiator(raw, network, local, allowed[string(remotePin)])
	} else {
		keys, peer, err = upgradeResponder(raw, network, local, allowed)
	}
	defer clear(keys.Send[:])
	defer clear(keys.Receive[:])
	if err != nil {
		return nil, err
	}
	send, err := chacha20poly1305.New(keys.Send[:])
	if err != nil {
		return nil, err
	}
	receive, err := chacha20poly1305.New(keys.Receive[:])
	if err != nil {
		return nil, err
	}
	if err = raw.SetDeadline(time.Time{}); err != nil {
		return nil, err
	}
	return &Conn{raw: raw, peer: bytes.Clone(peer), send: send, receive: receive}, nil
}

func upgradeInitiator(raw net.Conn, network string, local *Identity, pin []byte) (SessionKeys, []byte, error) {
	i, h, err := NewInitiator(network, local, pin)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	defer i.Close()
	if err = writeHandshake(raw, helloType, encodeHello(h)); err != nil {
		return SessionKeys{}, nil, err
	}
	payload, err := readHandshake(raw, offerType)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	pending, response, err := i.Respond(Offer{KEMPublicKey: payload[:mlkem.EncapsulationKeySize768], Signature: payload[mlkem.EncapsulationKeySize768:]})
	if err != nil {
		return SessionKeys{}, nil, err
	}
	defer pending.Close()
	if err = writeHandshake(raw, responseType, append(response.Ciphertext, response.Signature...)); err != nil {
		return SessionKeys{}, nil, err
	}
	payload, err = readHandshake(raw, serverFinishedType)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	var confirmation Confirmation
	copy(confirmation[:], payload)
	keys, final, err := pending.Finish(confirmation)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	if err = writeHandshake(raw, clientFinishedType, final[:]); err != nil {
		clear(keys.Send[:])
		clear(keys.Receive[:])
		return SessionKeys{}, nil, err
	}
	return keys, h.Responder, nil
}

func upgradeResponder(raw net.Conn, network string, local *Identity, pins map[string][]byte) (SessionKeys, []byte, error) {
	payload, err := readHandshake(raw, helloType)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	h, err := decodeHello(payload)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	pin, found := pins[string(h.Initiator)]
	if !found {
		return SessionKeys{}, nil, ErrRejected
	}
	responder, offer, err := NewResponder(network, local, pin, h)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	defer responder.Close()
	if err = writeHandshake(raw, offerType, append(offer.KEMPublicKey, offer.Signature...)); err != nil {
		return SessionKeys{}, nil, err
	}
	payload, err = readHandshake(raw, responseType)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	pending, confirmation, err := responder.Confirm(Response{Ciphertext: payload[:mlkem.CiphertextSize768], Signature: payload[mlkem.CiphertextSize768:]})
	if err != nil {
		return SessionKeys{}, nil, err
	}
	defer pending.Close()
	if err = writeHandshake(raw, serverFinishedType, confirmation[:]); err != nil {
		return SessionKeys{}, nil, err
	}
	payload, err = readHandshake(raw, clientFinishedType)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	copy(confirmation[:], payload)
	keys, err := pending.Finish(confirmation)
	if err != nil {
		return SessionKeys{}, nil, err
	}
	return keys, h.Initiator, nil
}

func handshakeSize(kind byte, length int) bool {
	switch kind {
	case helloType:
		return length >= helloFixedSize+1 && length <= helloFixedSize+64
	case offerType:
		return length == mlkem.EncapsulationKeySize768+mldsa65.SignatureSize
	case responseType:
		return length == mlkem.CiphertextSize768+mldsa65.SignatureSize
	case serverFinishedType, clientFinishedType:
		return length == 32
	default:
		return false
	}
}
func writeHandshake(w io.Writer, kind byte, payload []byte) error {
	if !handshakeSize(kind, len(payload)) {
		return ErrRejected
	}
	var h [handshakeHeaderSize]byte
	copy(h[:4], "DYPH")
	h[4] = WireVersion
	h[5] = kind
	binary.BigEndian.PutUint16(h[6:], uint16(len(payload)))
	if err := writeFull(w, h[:]); err != nil {
		return err
	}
	return writeFull(w, payload)
}
func readHandshake(r io.Reader, kind byte) ([]byte, error) {
	var h [handshakeHeaderSize]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		return nil, err
	}
	n := int(binary.BigEndian.Uint16(h[6:]))
	if string(h[:4]) != "DYPH" || h[4] != WireVersion || h[5] != kind || !handshakeSize(kind, n) {
		return nil, ErrRejected
	}
	payload := make([]byte, n)
	if _, err := io.ReadFull(r, payload); err != nil {
		return nil, err
	}
	return payload, nil
}
func encodeHello(h Initiation) []byte {
	p := make([]byte, helloFixedSize+len(h.Network))
	p[0] = byte(len(h.Network))
	n := 1
	n += copy(p[n:], h.Network)
	n += copy(p[n:], h.Nonce[:])
	n += copy(p[n:], h.Initiator)
	copy(p[n:], h.Responder)
	return p
}
func decodeHello(p []byte) (Initiation, error) {
	if len(p) < helloFixedSize+1 || len(p) > helloFixedSize+64 || int(p[0])+helloFixedSize != len(p) {
		return Initiation{}, ErrRejected
	}
	h := Initiation{Network: string(p[1 : 1+int(p[0])])}
	n := 1 + int(p[0])
	copy(h.Nonce[:], p[n:n+32])
	n += 32
	h.Initiator = bytes.Clone(p[n : n+mldsa65.PublicKeySize])
	n += mldsa65.PublicKeySize
	h.Responder = bytes.Clone(p[n:])
	if !validHello(h) {
		return Initiation{}, ErrRejected
	}
	return h, nil
}
func writeFull(w io.Writer, p []byte) error {
	for len(p) > 0 {
		n, err := w.Write(p)
		if n < 0 || n > len(p) {
			return io.ErrShortWrite
		}
		p = p[n:]
		if err != nil {
			return err
		}
		if n == 0 {
			return io.ErrNoProgress
		}
	}
	return nil
}

// Record headers: D Y P R, version(1), reserved-zero(1), sequence(8 BE),
// plaintext-length(2 BE). The complete header is authenticated as AEAD AAD.
func recordHeader(sequence uint64, n int) [recordHeaderSize]byte {
	var h [recordHeaderSize]byte
	copy(h[:4], "DYPR")
	h[4] = WireVersion
	binary.BigEndian.PutUint64(h[6:14], sequence)
	binary.BigEndian.PutUint16(h[14:], uint16(n))
	return h
}
func recordNonce(sequence uint64) [chacha20poly1305.NonceSize]byte {
	var nonce [chacha20poly1305.NonceSize]byte
	binary.BigEndian.PutUint64(nonce[4:], sequence)
	return nonce
}

func (c *Conn) Write(p []byte) (written int, err error) {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	if c.terminal.Load() {
		return 0, net.ErrClosed
	}
	for len(p) > 0 {
		if c.terminal.Load() {
			return written, net.ErrClosed
		}
		if c.writeSequence >= MaxRecords {
			return written, c.fail(ErrRecordLimit)
		}
		n := min(len(p), MaxPlaintext)
		h := recordHeader(c.writeSequence, n)
		nonce := recordNonce(c.writeSequence)
		ciphertext := c.send.Seal(nil, nonce[:], p[:n], h[:])
		if err = writeFull(c.raw, h[:]); err != nil {
			return written, c.fail(err)
		}
		if err = writeFull(c.raw, ciphertext); err != nil {
			return written, c.fail(err)
		}
		c.writeSequence++
		written += n
		p = p[n:]
	}
	return written, nil
}
func (c *Conn) Read(p []byte) (int, error) {
	c.readMu.Lock()
	defer c.readMu.Unlock()
	if c.terminal.Load() {
		clear(c.pending)
		c.pending = nil
		return 0, net.ErrClosed
	}
	if len(p) == 0 {
		return 0, nil
	}
	if len(c.pending) > 0 {
		n := copy(p, c.pending)
		clear(c.pending[:n])
		c.pending = c.pending[n:]
		return n, nil
	}
	if c.readSequence >= MaxRecords {
		return 0, c.fail(ErrRecordLimit)
	}
	var h [recordHeaderSize]byte
	if _, err := io.ReadFull(c.raw, h[:]); err != nil {
		return 0, c.fail(err)
	}
	n := int(binary.BigEndian.Uint16(h[14:]))
	sequence := binary.BigEndian.Uint64(h[6:14])
	if string(h[:4]) != "DYPR" || h[4] != WireVersion || h[5] != 0 || sequence != c.readSequence || n == 0 || n > MaxPlaintext {
		return 0, c.fail(ErrRecord)
	}
	encrypted := make([]byte, n+c.receive.Overhead())
	if _, err := io.ReadFull(c.raw, encrypted); err != nil {
		return 0, c.fail(err)
	}
	nonce := recordNonce(sequence)
	plaintext, err := c.receive.Open(encrypted[:0], nonce[:], encrypted, h[:])
	if err != nil {
		clear(encrypted)
		return 0, c.fail(ErrRecord)
	}
	c.readSequence++
	copied := copy(p, plaintext)
	clear(plaintext[:copied])
	c.pending = plaintext[copied:]
	return copied, nil
}
