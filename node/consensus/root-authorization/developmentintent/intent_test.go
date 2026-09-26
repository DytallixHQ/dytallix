package developmentintent

import (
	"bytes"
	"crypto/rand"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"sync"
	"testing"

	"github.com/cloudflare/circl/sign/slhdsa"
	root "github.com/dytallix/root-authorization"
)

func hash(c string) string { return strings.Repeat(c, 128) }
func marshal(t *testing.T, v any) []byte {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	return b
}
func fixture(t *testing.T, action root.Action) (Artifact, TrustedPolicy, []byte) {
	t.Helper()
	a := Artifact{Version: Version, Mode: Mode, ChainID: "local-intent-test", Action: action,
		IntentID: hash("1"), CurrentRelease: hash("2"), ApprovalRecord: hash("3"), ActivationHeight: 15}
	p := TrustedPolicy{Mode: Mode, ChainID: a.ChainID, Action: action, FinalizedHeight: 10,
		CurrentRelease: a.CurrentRelease, CurrentAnchor: hash("6"), StateSchema: 1, ApprovalRecord: a.ApprovalRecord,
		NotBeforeHeight: 10, NotAfterHeight: 12, MinimumActivationHeight: 15,
		MaximumActivationHeight: 18, MaxArtifactBytes: HardArtifactLimit}
	if action == root.Upgrade {
		a.Upgrade = &Upgrade{NextRelease: hash("4"), SourceSchema: 1, TargetSchema: 2, Migration: hash("5"), LastRollbackHeight: 20}
	} else {
		p.CurrentAnchor = hash("6")
		a.Halt = &Halt{Scope: "consensus", AnchorHeight: 10, Anchor: p.CurrentAnchor, ReviewHeight: 20, Incident: hash("7")}
	}
	b := marshal(t, a)
	p.ApprovedArtifactDigest = root.DigestArtifact(b)
	return a, p, b
}

func TestVersionedArtifactValidation(t *testing.T) {
	for _, action := range []root.Action{root.Upgrade, root.Emergency} {
		t.Run(string(action), func(t *testing.T) {
			_, p, b := fixture(t, action)
			if _, err := validateArtifact(b, p); err != nil {
				t.Fatal(err)
			}
		})
	}
	cases := map[string]func(*Artifact){
		"unknown version":            func(a *Artifact) { a.Version++ },
		"production":                 func(a *Artifact) { a.Mode = "production" },
		"zero intent ID":             func(a *Artifact) { a.IntentID = hash("0") },
		"noncanonical digest":        func(a *Artifact) { a.IntentID = hash("A") },
		"wrong chain":                func(a *Artifact) { a.ChainID = "other" },
		"wrong action":               func(a *Artifact) { a.Action = root.Emergency },
		"wrong release":              func(a *Artifact) { a.CurrentRelease = hash("8") },
		"wrong approval":             func(a *Artifact) { a.ApprovalRecord = hash("8") },
		"early activation":           func(a *Artifact) { a.ActivationHeight = 14 },
		"late activation":            func(a *Artifact) { a.ActivationHeight = 19 },
		"missing body":               func(a *Artifact) { a.Upgrade = nil },
		"two bodies":                 func(a *Artifact) { a.Halt = &Halt{} },
		"same release":               func(a *Artifact) { a.Upgrade.NextRelease = a.CurrentRelease },
		"wrong source schema":        func(a *Artifact) { a.Upgrade.SourceSchema++ },
		"missing target schema":      func(a *Artifact) { a.Upgrade.TargetSchema = 0 },
		"missing migration":          func(a *Artifact) { a.Upgrade.Migration = "" },
		"rollback before activation": func(a *Artifact) { a.Upgrade.LastRollbackHeight = 14 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			a, p, _ := fixture(t, root.Upgrade)
			mutate(&a)
			b := marshal(t, a)
			p.ApprovedArtifactDigest = root.DigestArtifact(b) // Exercise schema, not stale commitment.
			if _, err := validateArtifact(b, p); err == nil {
				t.Fatal("accepted invalid artifact")
			}
		})
	}
	for name, mutate := range map[string]func(*Halt){
		"resume scope":             func(h *Halt) { h.Scope = "resume" },
		"wrong anchor height":      func(h *Halt) { h.AnchorHeight++ },
		"wrong anchor digest":      func(h *Halt) { h.Anchor = hash("8") },
		"review before activation": func(h *Halt) { h.ReviewHeight = 14 },
		"missing incident":         func(h *Halt) { h.Incident = "" },
	} {
		t.Run(name, func(t *testing.T) {
			a, p, _ := fixture(t, root.Emergency)
			mutate(a.Halt)
			b := marshal(t, a)
			p.ApprovedArtifactDigest = root.DigestArtifact(b)
			if _, err := validateArtifact(b, p); err == nil {
				t.Fatal("accepted invalid halt")
			}
		})
	}
}

func TestCanonicalEncoding(t *testing.T) {
	_, p, b := fixture(t, root.Upgrade)
	for name, value := range map[string][]byte{
		"whitespace":       append(bytes.Clone(b), '\n'),
		"duplicate":        bytes.Replace(b, []byte(`"version":1`), []byte(`"version":1,"version":1`), 1),
		"unknown":          append(append([]byte(`{"unknown":0,`), b[1:]...), []byte{}...),
		"missing":          bytes.Replace(b, []byte(`,"halt":null`), nil, 1),
		"numeric spelling": bytes.Replace(b, []byte(`"version":1`), []byte(`"version":1.0`), 1),
		"escaped field":    bytes.Replace(b, []byte(`"version"`), []byte(`"\u0076ersion"`), 1),
		"extra document":   append(bytes.Clone(b), []byte(`{}`)...),
		"truncated":        b[:len(b)-1],
		"oversized":        bytes.Repeat([]byte(" "), HardArtifactLimit+1),
	} {
		t.Run(name, func(t *testing.T) {
			q := p
			q.ApprovedArtifactDigest = root.DigestArtifact(value)
			if _, err := validateArtifact(value, q); err == nil {
				t.Fatal("accepted alternate encoding")
			}
		})
	}
}

func TestExplicitTrustedPolicy(t *testing.T) {
	for name, mutate := range map[string]func(*TrustedPolicy){
		"production":                func(p *TrustedPolicy) { p.Mode = "production" },
		"zero policy":               func(p *TrustedPolicy) { *p = TrustedPolicy{} },
		"missing byte bound":        func(p *TrustedPolicy) { p.MaxArtifactBytes = 0 },
		"large byte bound":          func(p *TrustedPolicy) { p.MaxArtifactBytes = HardArtifactLimit + 1 },
		"small byte bound":          func(p *TrustedPolicy) { p.MaxArtifactBytes = 8 },
		"missing digest":            func(p *TrustedPolicy) { p.ApprovedArtifactDigest = [64]byte{} },
		"mismatched digest":         func(p *TrustedPolicy) { p.ApprovedArtifactDigest[0] ^= 1 },
		"missing schema":            func(p *TrustedPolicy) { p.StateSchema = 0 },
		"missing release":           func(p *TrustedPolicy) { p.CurrentRelease = "" },
		"missing approval":          func(p *TrustedPolicy) { p.ApprovalRecord = "" },
		"wrong action":              func(p *TrustedPolicy) { p.Action = root.Genesis },
		"early height":              func(p *TrustedPolicy) { p.NotBeforeHeight = 11 },
		"expired height":            func(p *TrustedPolicy) { p.NotAfterHeight = 9 },
		"past activation":           func(p *TrustedPolicy) { p.MinimumActivationHeight = 9 },
		"reverse activation window": func(p *TrustedPolicy) { p.MaximumActivationHeight = 14 },
		"missing anchor":            func(p *TrustedPolicy) { p.CurrentAnchor = "" },
	} {
		t.Run(name, func(t *testing.T) {
			_, p, b := fixture(t, root.Upgrade)
			mutate(&p)
			if err := Record(nil, p, root.Envelope{}, nil, b); err == nil {
				t.Fatal("accepted invalid policy")
			}
			if _, err := validateArtifact(b, p); err == nil {
				t.Fatal("accepted invalid policy before verification")
			}
		})
	}
	_, p, b := fixture(t, root.Emergency)
	p.CurrentAnchor = ""
	if _, err := validateArtifact(b, p); err == nil {
		t.Fatal("accepted missing halt anchor")
	}
}

var keyOnce sync.Once
var testPublic, testPrivate []byte
var keyErr error

func keys(t *testing.T) ([]byte, []byte) {
	t.Helper()
	keyOnce.Do(func() {
		pub, priv, err := slhdsa.GenerateKey(rand.Reader, slhdsa.SHAKE_256s)
		if err != nil {
			keyErr = err
			return
		}
		testPublic, keyErr = pub.MarshalBinary()
		if keyErr == nil {
			testPrivate, keyErr = priv.MarshalBinary()
		}
	})
	if keyErr != nil {
		t.Fatal(keyErr)
	}
	return testPublic, testPrivate
}
func signed(t *testing.T, p TrustedPolicy, sequence uint64) (root.Envelope, []byte) {
	t.Helper()
	pub, priv := keys(t)
	e := root.Envelope{Version: root.Version, Profile: root.Profile, ChainID: p.ChainID, Action: p.Action,
		Sequence: sequence, NotBeforeHeight: p.NotBeforeHeight, NotAfterHeight: p.NotAfterHeight, ArtifactDigest: p.ApprovedArtifactDigest}
	sig, err := root.SignForPolicy(e, priv, root.Policy{TrustedPublicKey: pub, ChainID: p.ChainID, Action: p.Action,
		ExpectedSequence: sequence, CurrentHeight: p.FinalizedHeight, ExpectedArtifactDigest: p.ApprovedArtifactDigest})
	if err != nil {
		t.Fatal(err)
	}
	return e, sig
}
func initial(t *testing.T, p TrustedPolicy) root.ExecutionState {
	t.Helper()
	pub, _ := keys(t)
	b, err := InitialState(p.ChainID, p.CurrentRelease, p.CurrentAnchor, p.StateSchema)
	if err != nil {
		t.Fatal(err)
	}
	return root.ExecutionState{ChainID: p.ChainID, FinalizedHeight: p.FinalizedHeight, TrustedPublicKey: pub,
		Consumed: map[root.Action]uint64{root.Upgrade: 0, root.Emergency: 0}, Application: b}
}

// memoryStore is an atomic test fixture, not a durability implementation.
type memoryStore struct {
	state root.ExecutionState
	fail  bool
	calls int
}

var errWrite = errors.New("test write failure")

func (s *memoryStore) Update(f func(root.ExecutionState) (root.ExecutionState, error)) error {
	s.calls++
	var copy root.ExecutionState
	b, _ := json.Marshal(s.state)
	if err := json.Unmarshal(b, &copy); err != nil {
		return err
	}
	next, err := f(copy)
	if err != nil {
		return err
	}
	if s.fail {
		return errWrite
	}
	s.state = next
	return nil
}

func TestRecordIsAtomicAndHasNoActiveEffect(t *testing.T) {
	for _, action := range []root.Action{root.Upgrade, root.Emergency} {
		t.Run(string(action), func(t *testing.T) {
			_, p, b := fixture(t, action)
			e, sig := signed(t, p, 1)
			s := &memoryStore{state: initial(t, p), fail: true}
			before := marshal(t, s.state)
			if err := Record(s, p, e, sig, b); !errors.Is(err, errWrite) {
				t.Fatalf("write failure: %v", err)
			}
			if !bytes.Equal(before, marshal(t, s.state)) {
				t.Fatal("partial mutation on failed commit")
			}
			s.fail = false
			if err := Record(s, p, e, sig, b); err != nil {
				t.Fatal(err)
			}
			var state State
			if err := json.Unmarshal(s.state.Application, &state); err != nil {
				t.Fatal(err)
			}
			if state.Pending == nil || state.Pending.Sequence != 1 || state.CurrentRelease != p.CurrentRelease || state.StateSchema != p.StateSchema || s.state.Consumed[action] != 1 {
				t.Fatal("pending receipt and sequence did not commit together, or active state changed")
			}
			next := marshal(t, s.state)
			duplicate := &memoryStore{state: initial(t, p)}
			if err := Record(duplicate, p, e, sig, b); err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(next, marshal(t, duplicate.state)) {
				t.Fatal("nondeterministic transition")
			}
			if err := Record(s, p, e, sig, b); err == nil {
				t.Fatal("replay accepted")
			}
			e2, sig2 := signed(t, p, 2)
			if err := Record(s, p, e2, sig2, b); !errors.Is(err, ErrPending) {
				t.Fatalf("pending overwrite: %v", err)
			}
			if !bytes.Equal(next, marshal(t, s.state)) {
				t.Fatal("replay or overwrite changed durable state")
			}
		})
	}
}

func TestRecordRejectsUntrustedOrChangedState(t *testing.T) {
	_, p, b := fixture(t, root.Upgrade)
	e, sig := signed(t, p, 1)
	for name, mutate := range map[string]func(*root.ExecutionState){
		"stale finalized height": func(s *root.ExecutionState) { s.FinalizedHeight++ },
		"changed finalized anchor": func(s *root.ExecutionState) {
			s.Application = bytes.Replace(s.Application, []byte(hash("6")), []byte(hash("8")), 1)
		},
		"revoked authority":    func(s *root.ExecutionState) { s.Revoked = true },
		"wrong root":           func(s *root.ExecutionState) { s.TrustedPublicKey = bytes.Repeat([]byte{1}, root.PublicKeySize) },
		"unknown action state": func(s *root.ExecutionState) { delete(s.Consumed, root.Upgrade) },
		"changed active release": func(s *root.ExecutionState) {
			s.Application = bytes.Replace(s.Application, []byte(hash("2")), []byte(hash("8")), 1)
		},
		"changed schema": func(s *root.ExecutionState) {
			s.Application = bytes.Replace(s.Application, []byte(`"state_schema":1`), []byte(`"state_schema":2`), 1)
		},
		"production state": func(s *root.ExecutionState) {
			s.Application = bytes.Replace(s.Application, []byte(Mode), []byte("production"), 1)
		},
		"noncanonical state": func(s *root.ExecutionState) { s.Application = append(s.Application, '\n') },
	} {
		t.Run(name, func(t *testing.T) {
			s := &memoryStore{state: initial(t, p)}
			mutate(&s.state)
			before := marshal(t, s.state)
			if err := Record(s, p, e, sig, b); err == nil {
				t.Fatal("changed state accepted")
			}
			if !bytes.Equal(before, marshal(t, s.state)) {
				t.Fatal("changed state after rejection")
			}
		})
	}
	t.Run("window from submission", func(t *testing.T) {
		s := &memoryStore{state: initial(t, p)}
		altered := e
		altered.NotAfterHeight++
		if err := Record(s, p, altered, sig, b); !errors.Is(err, ErrPolicy) || s.calls != 0 {
			t.Fatal("unapproved window reached store")
		}
	})
	t.Run("invalid signature", func(t *testing.T) {
		s := &memoryStore{state: initial(t, p)}
		bad := bytes.Clone(sig)
		bad[0] ^= 1
		before := marshal(t, s.state)
		if err := Record(s, p, e, bad, b); !errors.Is(err, root.ErrSignature) {
			t.Fatalf("signature: %v", err)
		}
		if !bytes.Equal(before, marshal(t, s.state)) {
			t.Fatal("signature rejection changed state")
		}
	})
}

// uncertainStore simulates a lost acknowledgement after the real store commits.
type uncertainStore struct{ store root.ExecutionStore }

func (s uncertainStore) Update(f func(root.ExecutionState) (root.ExecutionState, error)) error {
	if err := s.store.Update(f); err != nil {
		return err
	}
	return errWrite
}
func TestFileStoreRestartAfterUncertainAcknowledgement(t *testing.T) {
	for _, action := range []root.Action{root.Upgrade, root.Emergency} {
		t.Run(string(action), func(t *testing.T) {
			_, p, b := fixture(t, action)
			e, sig := signed(t, p, 1)
			dir := t.TempDir()
			if err := os.Chmod(dir, 0700); err != nil {
				t.Fatal(err)
			}
			s, err := root.CreateFileStore(dir, 65536, initial(t, p))
			if err != nil {
				t.Fatal(err)
			}
			if err := Record(uncertainStore{s}, p, e, sig, b); !errors.Is(err, errWrite) {
				t.Fatal("expected uncertain acknowledgement")
			}
			if err := s.Close(); err != nil {
				t.Fatal(err)
			}
			reopened, err := root.OpenFileStore(dir, 65536)
			if err != nil {
				t.Fatal(err)
			}
			defer reopened.Close()
			current, err := reopened.Read()
			if err != nil {
				t.Fatal(err)
			}
			var state State
			if err := json.Unmarshal(current.Application, &state); err != nil {
				t.Fatal(err)
			}
			if current.Consumed[action] != 1 || state.Pending == nil || state.Pending.Sequence != 1 {
				t.Fatal("receipt/sequence missing after reopen")
			}
			if err := Record(reopened, p, e, sig, b); err == nil {
				t.Fatal("replay after reopen accepted")
			}
		})
	}
}

func TestRecordAdmissionBounds(t *testing.T) {
	_, p, validArtifact := fixture(t, root.Upgrade)
	for name, artifact := range map[string][]byte{
		"empty":                nil,
		"above explicit bound": bytes.Repeat([]byte("x"), HardArtifactLimit+1),
	} {
		t.Run(name, func(t *testing.T) {
			store := &memoryStore{}
			if err := Record(store, p, root.Envelope{}, nil, artifact); !errors.Is(err, ErrSchema) {
				t.Fatalf("bound: %v", err)
			}
			if store.calls != 0 {
				t.Fatal("invalid artifact reached store")
			}
		})
	}
	for name, signature := range map[string][]byte{
		"missing signature": nil,
		"short signature":   make([]byte, root.SignatureSize-1),
		"long signature":    make([]byte, root.SignatureSize+1),
	} {
		t.Run(name, func(t *testing.T) {
			store := &memoryStore{}
			if err := Record(store, p, root.Envelope{}, signature, validArtifact); !errors.Is(err, root.ErrSignature) {
				t.Fatalf("signature bound: %v", err)
			}
			if store.calls != 0 {
				t.Fatal("invalid signature length reached store")
			}
		})
	}

}
