// Package pqcp2p implements a local experimental pinned PQC transport.
// The protocol has not received independent review. Production remains blocked.
package pqcp2p

import (
	"bytes"
	"crypto/hkdf"
	"crypto/hmac"
	"crypto/mlkem"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"sync"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

const Suite = "dytallix-pqcp2p-component-v1/mlkem768/mldsa65/hkdfsha256"

var ErrRejected = errors.New("PQC session establishment rejected")
var ErrConsumed = errors.New("PQC session state already consumed")
var ErrProductionBlocked = errors.New("NO GO: production transport profile, compiled boundary review and independent protocol review are incomplete")

// RequireProductionTransport always fails until a reviewed transport is integrated.
// A successful component test cannot change this gate.
func RequireProductionTransport() error { return ErrProductionBlocked }

// Identity is separate from validator, account, and exceptional authorization keys.
// Its private key is never included in a protocol message.
type Identity struct {
	private *mldsa65.PrivateKey
	public  *mldsa65.PublicKey
}

func GenerateIdentity() (*Identity, error) {
	p, s, e := mldsa65.GenerateKey(rand.Reader)
	if e != nil {
		return nil, e
	}
	return &Identity{s, p}, nil
}
func (i *Identity) PublicKey() []byte {
	if i == nil || i.public == nil {
		return nil
	}
	return i.public.Bytes()
}

// Initiation contains fresh randomness. Pins are full ML-DSA-65 public keys.
// The caller must obtain both pins from an authenticated configuration.
type Initiation struct {
	Network              string
	Nonce                [32]byte
	Initiator, Responder []byte
}
type Offer struct{ KEMPublicKey, Signature []byte }
type Response struct{ Ciphertext, Signature []byte }
type Confirmation [32]byte

// SessionKeys feed the experimental directional authenticated record layer.
type SessionKeys struct{ Send, Receive [32]byte }
type initiatorState struct {
	identity *Identity
	hello    Initiation
}
type Initiator struct {
	mu    sync.Mutex
	state *initiatorState
}
type responderState struct {
	hello Initiation
	offer Offer
	key   *mlkem.DecapsulationKey768
}
type Responder struct {
	mu    sync.Mutex
	state *responderState
}
type PendingInitiator struct {
	mu    sync.Mutex
	state *pendingState
}
type PendingResponder struct {
	mu    sync.Mutex
	state *pendingState
}
type pendingState struct {
	keys           SessionKeys
	server, client Confirmation
}

func cloneHello(h Initiation) Initiation {
	h.Initiator = bytes.Clone(h.Initiator)
	h.Responder = bytes.Clone(h.Responder)
	return h
}
func validHello(h Initiation) bool {
	return len(h.Network) > 0 && len(h.Network) <= 64 && len(h.Initiator) == mldsa65.PublicKeySize && len(h.Responder) == mldsa65.PublicKeySize && !bytes.Equal(h.Initiator, h.Responder) && h.Nonce != [32]byte{}
}

// NewInitiator creates one attempt. Retry requires a fresh attempt and nonce.
func NewInitiator(network string, local *Identity, pinnedResponder []byte) (*Initiator, Initiation, error) {
	if local == nil || local.private == nil {
		return nil, Initiation{}, ErrRejected
	}
	h := Initiation{Network: network, Initiator: local.PublicKey(), Responder: bytes.Clone(pinnedResponder)}
	if _, e := rand.Read(h.Nonce[:]); e != nil {
		return nil, Initiation{}, e
	}
	if !validHello(h) {
		return nil, Initiation{}, ErrRejected
	}
	return &Initiator{state: &initiatorState{local, cloneHello(h)}}, h, nil
}

// NewResponder authenticates the peer configuration before creating an ephemeral key.
// The offer signs the network, roles, nonce, full identity keys, suite, and KEM key.
func NewResponder(network string, local *Identity, pinnedInitiator []byte, h Initiation) (*Responder, Offer, error) {
	if local == nil || local.private == nil || !validHello(h) || network != h.Network || !bytes.Equal(pinnedInitiator, h.Initiator) || !bytes.Equal(local.PublicKey(), h.Responder) {
		return nil, Offer{}, ErrRejected
	}
	k, e := mlkem.GenerateKey768()
	if e != nil {
		return nil, Offer{}, e
	}
	o := Offer{KEMPublicKey: k.EncapsulationKey().Bytes()}
	o.Signature, e = sign(local, offerMessage(h, o), "responder-offer")
	if e != nil {
		return nil, Offer{}, e
	}
	saved := Offer{bytes.Clone(o.KEMPublicKey), bytes.Clone(o.Signature)}
	return &Responder{state: &responderState{cloneHello(h), saved, k}}, o, nil
}

// Respond consumes the initiator attempt, including on failure.
// Keys remain unavailable until the responder confirms its shared secret.
func (i *Initiator) Respond(o Offer) (*PendingInitiator, Response, error) {
	i.mu.Lock()
	defer i.mu.Unlock()
	s := i.state
	i.state = nil
	if s == nil {
		return nil, Response{}, ErrConsumed
	}
	if len(o.KEMPublicKey) != mlkem.EncapsulationKeySize768 || !verify(s.hello.Responder, offerMessage(s.hello, o), "responder-offer", o.Signature) {
		return nil, Response{}, ErrRejected
	}
	k, e := mlkem.NewEncapsulationKey768(o.KEMPublicKey)
	if e != nil {
		return nil, Response{}, ErrRejected
	}
	secret, ct := k.Encapsulate()
	defer clear(secret)
	r := Response{Ciphertext: ct}
	r.Signature, e = sign(s.identity, responseMessage(s.hello, o, r), "initiator-response")
	if e != nil {
		return nil, Response{}, e
	}
	p, e := derive(secret, s.hello, o, r, true)
	if e != nil {
		return nil, Response{}, e
	}
	return &PendingInitiator{state: p}, r, nil
}

// Confirm consumes the responder attempt and releases its ephemeral private key.
// It produces key confirmation but does not expose application keys yet.
func (s *Responder) Confirm(r Response) (*PendingResponder, Confirmation, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	st := s.state
	s.state = nil
	if st == nil {
		return nil, Confirmation{}, ErrConsumed
	}
	if len(r.Ciphertext) != mlkem.CiphertextSize768 || !verify(st.hello.Initiator, responseMessage(st.hello, st.offer, r), "initiator-response", r.Signature) {
		return nil, Confirmation{}, ErrRejected
	}
	secret, e := st.key.Decapsulate(r.Ciphertext)
	st.key = nil
	if e != nil {
		return nil, Confirmation{}, ErrRejected
	}
	defer clear(secret)
	p, e := derive(secret, st.hello, st.offer, r, false)
	if e != nil {
		return nil, Confirmation{}, e
	}
	return &PendingResponder{state: p}, p.server, nil
}

// Finish checks responder confirmation before exposing initiator keys.
func (p *PendingInitiator) Finish(c Confirmation) (SessionKeys, Confirmation, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	s := p.state
	p.state = nil
	if s == nil {
		return SessionKeys{}, Confirmation{}, ErrConsumed
	}
	defer s.clear()
	if !hmac.Equal(c[:], s.server[:]) {
		return SessionKeys{}, Confirmation{}, ErrRejected
	}
	return s.keys, s.client, nil
}

// Finish checks initiator confirmation before exposing responder keys.
func (p *PendingResponder) Finish(c Confirmation) (SessionKeys, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	s := p.state
	p.state = nil
	if s == nil {
		return SessionKeys{}, ErrConsumed
	}
	defer s.clear()
	if !hmac.Equal(c[:], s.client[:]) {
		return SessionKeys{}, ErrRejected
	}
	return s.keys, nil
}
func (p *pendingState) clear() {
	clear(p.keys.Send[:])
	clear(p.keys.Receive[:])
	clear(p.server[:])
	clear(p.client[:])
}

// encode uses length prefixes for unambiguous transcript fields.
func encode(parts ...[]byte) []byte {
	var b bytes.Buffer
	for _, p := range parts {
		var n [4]byte
		binary.BigEndian.PutUint32(n[:], uint32(len(p)))
		b.Write(n[:])
		b.Write(p)
	}
	return b.Bytes()
}
func offerMessage(h Initiation, o Offer) []byte {
	return encode([]byte(Suite), []byte(h.Network), h.Nonce[:], h.Initiator, h.Responder, o.KEMPublicKey)
}
func responseMessage(h Initiation, o Offer, r Response) []byte {
	return encode(offerMessage(h, o), o.Signature, r.Ciphertext)
}
func sign(i *Identity, msg []byte, role string) (sig []byte, err error) {
	defer func() {
		if recover() != nil {
			clear(sig)
			sig = nil
			err = ErrRejected
		}
	}()
	sig = make([]byte, mldsa65.SignatureSize)
	err = mldsa65.SignTo(i.private, msg, []byte(Suite+"/"+role), true, sig)
	return sig, err
}
func verify(raw, msg []byte, role string, sig []byte) bool {
	if len(raw) != mldsa65.PublicKeySize || len(sig) != mldsa65.SignatureSize {
		return false
	}
	var packed [mldsa65.PublicKeySize]byte
	copy(packed[:], raw)
	var k mldsa65.PublicKey
	k.Unpack(&packed)
	return mldsa65.Verify(&k, msg, []byte(Suite+"/"+role), sig)
}
func derive(secret []byte, h Initiation, o Offer, r Response, initiator bool) (*pendingState, error) {
	transcript := sha256.Sum256(encode(responseMessage(h, o, r), r.Signature))
	material, e := hkdf.Key(sha256.New, secret, transcript[:], Suite+"/traffic-and-confirmation", 128)
	if e != nil {
		return nil, e
	}
	defer clear(material)
	p := new(pendingState)
	copy(p.keys.Send[:], material[:32])
	copy(p.keys.Receive[:], material[32:64])
	if !initiator {
		p.keys.Send, p.keys.Receive = p.keys.Receive, p.keys.Send
	}
	server := hmac.New(sha256.New, material[64:96])
	server.Write(encode([]byte(Suite+"/server-finished"), transcript[:]))
	client := hmac.New(sha256.New, material[96:128])
	client.Write(encode([]byte(Suite+"/client-finished"), transcript[:]))
	copy(p.server[:], server.Sum(nil))
	copy(p.client[:], client.Sum(nil))
	return p, nil
}

// Close discards an unused attempt. It does not destroy the long-term identity.
func (i *Initiator) Close() { i.mu.Lock(); defer i.mu.Unlock(); i.state = nil }

// Close releases an unused ephemeral key. Go does not guarantee key-object erasure.
func (s *Responder) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state != nil {
		s.state.key = nil
	}
	s.state = nil
}

// Close clears pending derived keys. Call it when the attempt is abandoned.
func (p *PendingInitiator) Close() {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.state != nil {
		p.state.clear()
	}
	p.state = nil
}

// Close clears pending derived keys. Call it when the attempt is abandoned.
func (p *PendingResponder) Close() {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.state != nil {
		p.state.clear()
	}
	p.state = nil
}
