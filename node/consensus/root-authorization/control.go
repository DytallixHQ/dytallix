package rootauthorization

import (
	"bytes"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

// Root control requests (E05; node/docs/mainnet/control-signing.md). The
// node's dytallix-control prepares a request online; each custodian signs
// it offline with one key. The request's artifact is what is signed, so
// everything a signer is shown and checked is read from the artifact itself:
// the request's other fields only have to agree with it.
const (
	ControlRequestSchema   = "dytallix.control-request.v1"
	ControlSignatureSchema = "dytallix.control-signature.v1"
)

var ErrControl = errors.New("invalid root control request")

type ControlEnvelope struct {
	ChainID         string `json:"chain_id"`
	Action          string `json:"action"`
	Sequence        uint64 `json:"sequence"`
	NotBeforeHeight uint64 `json:"not_before_height"`
	NotAfterHeight  uint64 `json:"not_after_height"`
	ArtifactSHA512  string `json:"artifact_sha512"`
}

type ControlAuthority struct {
	Purpose       string         `json:"purpose"`
	Threshold     int            `json:"threshold"`
	MaxSignatures int            `json:"max_signatures"`
	Keys          []AuthorityKey `json:"keys"`
}

type ControlRequest struct {
	Schema        string           `json:"schema"`
	Operation     string           `json:"operation"`
	Kind          string           `json:"kind"`
	AnchorHeight  uint64           `json:"anchor_height"`
	AnchorAppHash string           `json:"anchor_app_hash"`
	Payload       json.RawMessage  `json:"payload"`
	ArtifactHex   string           `json:"artifact_hex"`
	Envelope      ControlEnvelope  `json:"envelope"`
	Authority     ControlAuthority `json:"authority"`
}

type ControlSignature struct {
	Schema         string `json:"schema"`
	KeyID          string `json:"key_id"`
	ArtifactSHA512 string `json:"artifact_sha512"`
	Sequence       uint64 `json:"sequence"`
	SignatureHex   string `json:"signature_hex"`
}

// ControlSummary is what the signer is shown, read from the artifact.
type ControlSummary struct {
	Operation      string
	ChainID        string
	Sequence       uint64
	AnchorHeight   uint64
	AnchorAppHash  string
	NotBefore      uint64
	NotAfter       uint64
	Details        [][2]string
	ArtifactSHA512 string
}

type controlKind struct {
	domain string
	action Action
	family string
}

var controlKinds = map[string]controlKind{
	"dytallix-emergency-control-v2": {"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v2\x00", Emergency, "emergency"},
	"dytallix-upgrade-control-v2":   {"DYTALLIX/CHAIN-UPGRADE/v2\x00", Upgrade, "upgrade"},
	"dytallix-release-handover-v2":  {"DYTALLIX/RELEASE-HANDOVER/v2\x00", Upgrade, "handover"},
	// Root kit replacement v1: the current upgrade keys sign, and the new
	// kit's keys sign the same request as their proofs of possession.
	ReplacementKind: {"DYTALLIX/ROOT-KIT-REPLACEMENT/v1\x00", Upgrade, "replacement"},
	// Restart v1: the upgrade keys switch the active release at the halted
	// height, built on a stopped node by dytallix-state-check.
	"dytallix-release-restart-v1": {"DYTALLIX/RELEASE-RESTART/v1\x00", Upgrade, "restart"},
}

// ReplacementKind is the kit replacement control's kind.
const ReplacementKind = "dytallix-root-kit-replacement-v1"

// replacementRoles are the roles a kit replacement changes, in payload order.
var replacementRoles = []string{"upgrade", "freeze", "resume"}

// field reads a nested field of the decoded payload.
func field(value any, keys ...string) any {
	for _, key := range keys {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		value = object[key]
	}
	return value
}

func textField(value any) (string, bool) {
	s, ok := value.(string)
	return s, ok && s != ""
}

func numberField(value any) (uint64, bool) {
	n, ok := value.(json.Number)
	if !ok {
		return 0, false
	}
	var out uint64
	if _, err := fmt.Sscan(string(n), &out); err != nil || fmt.Sprint(out) != string(n) {
		return 0, false
	}
	return out, true
}

// Check reads the artifact, checks that the request's envelope, operation
// and anchor agree with it, and returns the envelope to sign and the summary
// to show.
func (r ControlRequest) Check() (Envelope, ControlSummary, error) {
	fail := func(what string) (Envelope, ControlSummary, error) {
		return Envelope{}, ControlSummary{}, fmt.Errorf("%w: %s", ErrControl, what)
	}
	if r.Schema != ControlRequestSchema {
		return fail("unsupported request schema")
	}
	kind, ok := controlKinds[r.Kind]
	if !ok {
		return fail("unknown control kind " + r.Kind)
	}
	artifact, err := hex.DecodeString(r.ArtifactHex)
	if err != nil || hex.EncodeToString(artifact) != r.ArtifactHex {
		return fail("the artifact is not lowercase hex")
	}
	digest := sha512.Sum512(artifact)
	if hex.EncodeToString(digest[:]) != r.Envelope.ArtifactSHA512 {
		return fail("the envelope's digest is not the artifact's")
	}
	if !bytes.HasPrefix(artifact, []byte(kind.domain)) {
		return fail("the artifact is not a " + r.Kind + " control")
	}
	decoder := json.NewDecoder(bytes.NewReader(artifact[len(kind.domain):]))
	decoder.UseNumber()
	var payload map[string]any
	if err := decoder.Decode(&payload); err != nil || decoder.More() {
		return fail("the artifact payload is not one JSON object")
	}
	summary := ControlSummary{ArtifactSHA512: r.Envelope.ArtifactSHA512}
	var ok1, ok2, ok3, ok4, ok5, ok6 bool
	summary.ChainID, ok1 = textField(payload["chain_id"])
	summary.Sequence, ok2 = numberField(payload["sequence"])
	add := func(name string, value any) {
		if s, ok := value.(string); ok && s != "" {
			summary.Details = append(summary.Details, [2]string{name, s})
		} else if n, ok := value.(json.Number); ok {
			summary.Details = append(summary.Details, [2]string{name, string(n)})
		}
	}
	switch kind.family {
	case "emergency":
		action, _ := textField(payload["action"])
		summary.Operation = action
		summary.AnchorHeight, ok3 = numberField(payload["parent_height"])
		summary.AnchorAppHash, ok4 = textField(payload["parent_app_hash"])
		summary.NotBefore, ok5 = numberField(field(payload, "v2", "not_before_height"))
		summary.NotAfter, ok6 = numberField(field(payload, "v2", "not_after_height"))
		add("release_sha512", payload["release_sha512"])
		add("authority_epoch", field(payload, "v2", "authority_epoch"))
		add("incident_sha256", field(payload, "v2", "incident_sha256"))
		add("freeze_receipt_sha256", field(payload, "v2", "resume", "freeze_receipt_sha256"))
		add("restored_state_sha256", field(payload, "v2", "resume", "restored_state_sha256"))
		add("readiness_evidence_sha256", field(payload, "v2", "resume", "readiness_evidence_sha256"))
		if action != "freeze" && action != "resume" {
			return fail("unknown emergency action")
		}
	case "restart":
		// Signed for the halted height alone: the window is that one block.
		summary.Operation = "restart"
		summary.AnchorHeight, ok3 = numberField(payload["parent_height"])
		summary.AnchorAppHash, ok4 = textField(payload["parent_app_hash"])
		summary.NotBefore, ok5 = numberField(payload["halted_height"])
		summary.NotAfter = summary.NotBefore
		ok6 = ok3 && ok5 && summary.NotBefore == summary.AnchorHeight+1
		add("source_release_sha512", payload["source_release_sha512"])
		add("target_release_sha512", payload["target_release_sha512"])
		add("authority_epoch", payload["authority_epoch"])
		add("state_schema", payload["state_schema"])
		if payload["halted_block_hash"] == nil {
			add("halted_block_hash", "none: block H was never decided")
		} else {
			add("halted_block_hash", payload["halted_block_hash"])
		}
		add("emergency_receipt_sha256", payload["emergency_receipt_sha256"])
		add("pending_admission_receipt_sha256", payload["pending_admission_receipt_sha256"])
		add("evidence_sha256", payload["evidence_sha256"])
	case "replacement":
		summary.Operation = "kit-replacement"
		summary.AnchorHeight, ok3 = numberField(payload["anchor_height"])
		summary.AnchorAppHash, ok4 = textField(payload["anchor_app_hash"])
		summary.NotBefore, ok5 = numberField(payload["not_before_height"])
		summary.NotAfter, ok6 = numberField(payload["not_after_height"])
		add("authority_epoch", payload["authority_epoch"])
		for _, role := range replacementRoles {
			leaving, ok := textField(field(payload, "replaced", role))
			id, okID := textField(field(payload, "keys", role, "key_id"))
			publicHex, okKey := textField(field(payload, "keys", role, "public_key_hex"))
			public, okHex := canonicalHex(publicHex, PublicKeySize)
			if !ok || !okID || !okKey || !okHex || KeyID(public) != id {
				return fail("the replacement's " + role + " keys are missing or inconsistent")
			}
			// The new keys sign their proofs, so the request lists them.
			listed := false
			for _, key := range r.Authority.Keys {
				listed = listed || (key.KeyID == id && key.PublicKeyHex == publicHex)
			}
			if !listed {
				return fail("the request does not list the new " + role + " key")
			}
			add("leaving_"+role, leaving)
			add("new_"+role, id)
		}
	default:
		action, _ := textField(field(payload, "action", "action"))
		summary.Operation = kind.family + "-" + action
		if action != "admit" && action != "activate" && action != "cancel" {
			return fail("unknown " + kind.family + " action")
		}
		if kind.family == "upgrade" {
			summary.AnchorHeight, ok3 = numberField(payload["anchor_height"])
			summary.AnchorAppHash, ok4 = textField(payload["anchor_app_hash"])
			summary.NotBefore, ok5 = numberField(payload["not_before_height"])
			summary.NotAfter, ok6 = numberField(payload["not_after_height"])
		} else {
			summary.AnchorHeight, ok3 = numberField(payload["parent_height"])
			summary.AnchorAppHash, ok4 = textField(payload["parent_app_hash"])
			summary.NotBefore, ok5 = numberField(field(payload, "v2", "not_before_height"))
			summary.NotAfter, ok6 = numberField(field(payload, "v2", "not_after_height"))
			add("transition", field(payload, "action", "plan", "transition", "transition"))
		}
		add("source_release_sha512", payload["source_release_sha512"])
		add("target_release_sha512", field(payload, "action", "plan", "target_release_sha512"))
		add("authority_epoch", payload["authority_epoch"])
		add("authorization_sha256", field(payload, "action", "plan", "authorization_sha256"))
		add("evidence_sha256", field(payload, "action", "evidence_sha256"))
		add("emergency_receipt_sha256", field(payload, "action", "emergency_receipt_sha256"))
		add("upgrade_activation_sha256", field(payload, "action", "upgrade_activation_sha256"))
		add("plan_sha256", field(payload, "action", "plan_sha256"))
	}
	if !(ok1 && ok2 && ok3 && ok4 && ok5 && ok6) || !validChainID(summary.ChainID) || summary.Sequence == 0 ||
		summary.NotBefore > summary.NotAfter || summary.NotBefore <= summary.AnchorHeight {
		return fail("the artifact's chain, sequence, anchor or window is missing or invalid")
	}
	e := r.Envelope
	if e.ChainID != summary.ChainID || e.Action != string(kind.action) || e.Sequence != summary.Sequence ||
		e.NotBeforeHeight != summary.NotBefore || e.NotAfterHeight != summary.NotAfter {
		return fail("the envelope differs from the artifact")
	}
	if r.Operation != summary.Operation || r.AnchorHeight != summary.AnchorHeight || r.AnchorAppHash != summary.AnchorAppHash {
		return fail("the request's operation or anchor differs from the artifact")
	}
	return Envelope{Version: Version, Profile: Profile, ChainID: e.ChainID, Action: kind.action, Sequence: e.Sequence,
		NotBeforeHeight: e.NotBeforeHeight, NotAfterHeight: e.NotAfterHeight, ArtifactDigest: digest}, summary, nil
}

// controlPolicy is the signer's own policy for one key: the envelope it
// checked, at the window's first height.
func controlPolicy(e Envelope, public []byte) Policy {
	return Policy{TrustedPublicKey: public, ChainID: e.ChainID, Action: e.Action, ExpectedSequence: e.Sequence,
		CurrentHeight: e.NotBeforeHeight, ExpectedArtifactDigest: e.ArtifactDigest}
}

func authorityPublicKey(r ControlRequest, key AuthorityKey) ([]byte, error) {
	public, ok := canonicalHex(key.PublicKeyHex, PublicKeySize)
	if !ok || KeyID(public) != key.KeyID {
		return nil, fmt.Errorf("%w: the public key record is inconsistent", ErrControl)
	}
	for _, listed := range r.Authority.Keys {
		if listed.KeyID == key.KeyID && listed.PublicKeyHex == key.PublicKeyHex {
			return public, nil
		}
	}
	return nil, fmt.Errorf("%w: key %s is not one of this control's %s keys", ErrControl, key.KeyID, r.Authority.Purpose)
}

// SignControl checks the request and signs its envelope with one key.
func SignControl(r ControlRequest, privateKey []byte, key AuthorityKey) (ControlSignature, error) {
	e, _, err := r.Check()
	if err != nil {
		return ControlSignature{}, err
	}
	public, err := authorityPublicKey(r, key)
	if err != nil {
		return ControlSignature{}, err
	}
	signature, err := SignForPolicy(e, privateKey, controlPolicy(e, public))
	if err != nil {
		return ControlSignature{}, err
	}
	return ControlSignature{Schema: ControlSignatureSchema, KeyID: key.KeyID, ArtifactSHA512: r.Envelope.ArtifactSHA512,
		Sequence: e.Sequence, SignatureHex: hex.EncodeToString(signature)}, nil
}

// VerifyControlSignature checks one signature file against the request and
// the signer's public key, as the node's helper will.
func VerifyControlSignature(r ControlRequest, signature ControlSignature, key AuthorityKey) error {
	e, _, err := r.Check()
	if err != nil {
		return err
	}
	public, err := authorityPublicKey(r, key)
	if err != nil {
		return err
	}
	raw, ok := canonicalHex(signature.SignatureHex, SignatureSize)
	if signature.Schema != ControlSignatureSchema || signature.KeyID != key.KeyID || !ok ||
		signature.ArtifactSHA512 != r.Envelope.ArtifactSHA512 || signature.Sequence != e.Sequence {
		return fmt.Errorf("%w: the signature file is for another key or control", ErrControl)
	}
	return Verify(e, raw, controlPolicy(e, public))
}

// Render is the summary as the signer sees it.
func (s ControlSummary) Render() string {
	var b strings.Builder
	fmt.Fprintf(&b, "operation        %s\nchain            %s\nsequence         %d\n", s.Operation, s.ChainID, s.Sequence)
	fmt.Fprintf(&b, "anchor           %d %s\nwindow           %d to %d\n", s.AnchorHeight, s.AnchorAppHash, s.NotBefore, s.NotAfter)
	for _, d := range s.Details {
		fmt.Fprintf(&b, "%-16s %s\n", d[0], d[1])
	}
	fmt.Fprintf(&b, "artifact_sha512  %s\n", s.ArtifactSHA512)
	return b.String()
}
