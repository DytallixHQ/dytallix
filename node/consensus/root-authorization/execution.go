package rootauthorization

import (
	"bytes"
	"crypto/sha512"
	"errors"
	"math"
)

var (
	ErrExecutionState    = errors.New("invalid root execution state")
	ErrRevoked           = errors.New("root authority is revoked")
	ErrSequenceExhausted = errors.New("root sequence exhausted")
)

// ExecutionState belongs to the trusted application database. Consumed entries
// must be initialized explicitly for permitted actions. Application is the
// caller's state encoding. FinalizedHeight shares its consensus transaction.
// State contains no signing key. Key replacement, revocation, reactivation and
// restore must preserve consumed sequences without decrease or reset. These
// operations require separately approved custody and governance procedures.
type ExecutionState struct {
	ChainID          string
	FinalizedHeight  uint64
	TrustedPublicKey []byte
	Revoked          bool
	Consumed         map[Action]uint64
	Application      []byte
}

// ExecutionStore serializes each Update with every authority and application
// update. It supplies a consistent current state and atomically persists the
// returned state, including Application and Consumed, or persists neither.
// It MUST discard callback changes on error, provide durable commit semantics,
// and prevent concurrent writers from bypassing this transaction boundary.
// A commit acknowledgement can be lost: callers must read durable state before
// retrying. Execute does not infer success from an uncertain storage error.
// FileStore supplies a complete local document adapter on supported systems.
// A production chain database adapter is not supplied by this package.
type ExecutionStore interface {
	Update(func(ExecutionState) (ExecutionState, error)) error
}

// ExecutionIntent must come from independently trusted approval and finalized
// state. Never construct it from the submitted envelope or an untrusted clock.
// The store supplies the currently trusted public key and next sequence.
type ExecutionIntent struct {
	ChainID         string
	Action          Action
	ArtifactDigest  [sha512.Size]byte
	FinalizedHeight uint64
}

// ArtifactTransition validates the caller's reviewed artifact schema and returns
// new application bytes. It MUST be deterministic and have no external effects.
// It must not perform file replacement, deployment, signing, RPC, or nested store
// updates. Those operations cannot share the atomic database transaction here.
// Input slices are private copies; output ownership transfers to Execute.
type ArtifactTransition func(application, artifact []byte) ([]byte, error)

// Execute verifies a root action inside the same transaction that changes the
// caller's application state and consumes the sequence. It does not define any
// genesis, upgrade, emergency, custody or key-rotation operation. The caller must
// supply the reviewed transition and production store adapter before activation.
func Execute(store ExecutionStore, intent ExecutionIntent, e Envelope, signature, artifact []byte, transition ArtifactTransition) error {
	if store == nil || transition == nil || !validChainID(intent.ChainID) || !validAction(intent.Action) {
		return ErrPolicy
	}
	signature = bytes.Clone(signature)
	artifact = bytes.Clone(artifact)
	if DigestArtifact(artifact) != intent.ArtifactDigest {
		return ErrPolicy
	}
	return store.Update(func(current ExecutionState) (ExecutionState, error) {
		reject := func(err error) (ExecutionState, error) { return ExecutionState{}, err }
		if current.ChainID != intent.ChainID || current.FinalizedHeight != intent.FinalizedHeight {
			return reject(ErrExecutionState)
		}
		if current.Revoked {
			return reject(ErrRevoked)
		}
		last, exists := current.Consumed[intent.Action]
		if !exists {
			return reject(ErrExecutionState)
		}
		if last == math.MaxUint64 {
			return reject(ErrSequenceExhausted)
		}
		key := bytes.Clone(current.TrustedPublicKey)
		policy := Policy{TrustedPublicKey: key, ChainID: intent.ChainID, Action: intent.Action,
			ExpectedSequence: last + 1, CurrentHeight: current.FinalizedHeight, ExpectedArtifactDigest: intent.ArtifactDigest}
		if err := Verify(e, signature, policy); err != nil {
			return reject(err)
		}
		nextApplication, err := transition(bytes.Clone(current.Application), bytes.Clone(artifact))
		if err != nil {
			return reject(err)
		}
		next := ExecutionState{ChainID: current.ChainID, FinalizedHeight: current.FinalizedHeight, TrustedPublicKey: key, Revoked: current.Revoked,
			Consumed: make(map[Action]uint64, len(current.Consumed)), Application: bytes.Clone(nextApplication)}
		for action, sequence := range current.Consumed {
			next.Consumed[action] = sequence
		}
		next.Consumed[intent.Action] = e.Sequence
		return next, nil
	})
}
