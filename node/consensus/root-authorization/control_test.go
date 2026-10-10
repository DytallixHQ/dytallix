package rootauthorization

import (
	"bytes"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
)

// testControl builds a freeze request as dytallix-control writes one, for
// five freeze keys derived from test kits.
func testControl(t *testing.T) (ControlRequest, [][]byte, []AuthorityKey) {
	t.Helper()
	var privates [][]byte
	var keys []AuthorityKey
	for kit := 1; kit <= KitCount; kit++ {
		public, private, err := DeriveKitKey(testSecret(byte(40+kit)), kit, "freeze")
		if err != nil {
			t.Fatal(err)
		}
		privates = append(privates, private)
		keys = append(keys, AuthorityKey{KeyID: KeyID(public), PublicKeyHex: hex.EncodeToString(public)})
	}
	payload := `{"schema":2,"chain_id":"dytallix-staging-1","release_sha512":"` + strings.Repeat("a", 128) +
		`","action":"freeze","sequence":4,"parent_height":100,"parent_app_hash":"` + strings.Repeat("b", 64) +
		`","target_height":101,"v2":{"genesis_sha256":"` + strings.Repeat("c", 64) + `","authority_epoch":1,"policy_sha256":"` +
		strings.Repeat("d", 64) + `","not_before_height":101,"not_after_height":34660,"incident_sha256":"` + strings.Repeat("e", 64) + `"}}`
	artifact := append([]byte("DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v2\x00"), payload...)
	digest := sha512.Sum512(artifact)
	return ControlRequest{
		Schema: ControlRequestSchema, Operation: "freeze", Kind: "dytallix-emergency-control-v2",
		AnchorHeight: 100, AnchorAppHash: strings.Repeat("b", 64), Payload: json.RawMessage(payload),
		ArtifactHex: hex.EncodeToString(artifact),
		Envelope: ControlEnvelope{ChainID: "dytallix-staging-1", Action: "emergency", Sequence: 4,
			NotBeforeHeight: 101, NotAfterHeight: 34660, ArtifactSHA512: hex.EncodeToString(digest[:])},
		Authority: ControlAuthority{Purpose: "freeze", Threshold: 3, MaxSignatures: 3, Keys: keys},
	}, privates, keys
}

func TestControlSignAndVerify(t *testing.T) {
	request, privates, keys := testControl(t)
	envelope, summary, err := request.Check()
	if err != nil {
		t.Fatal(err)
	}
	if summary.Operation != "freeze" || summary.Sequence != 4 || summary.NotBefore != 101 || summary.NotAfter != 34660 ||
		envelope.Action != Emergency || envelope.NotBeforeHeight != 101 {
		t.Fatalf("unexpected summary %+v", summary)
	}
	if rendered := summary.Render(); !strings.Contains(rendered, "incident_sha256  "+strings.Repeat("e", 64)) {
		t.Fatal(rendered)
	}
	signature, err := SignControl(request, privates[1], keys[1])
	if err != nil {
		t.Fatal(err)
	}
	if signature.KeyID != keys[1].KeyID || signature.Sequence != 4 || signature.ArtifactSHA512 != request.Envelope.ArtifactSHA512 {
		t.Fatalf("unexpected signature record %+v", signature)
	}
	if err := VerifyControlSignature(request, signature, keys[1]); err != nil {
		t.Fatal(err)
	}
	if VerifyControlSignature(request, signature, keys[2]) == nil {
		t.Fatal("a signature verified under another key")
	}
	// The private key must be the record's.
	if _, err := SignControl(request, privates[2], keys[1]); err == nil {
		t.Fatal("signed with a private key that is not the record's")
	}
	// Only the request's authority signs.
	other, _, _ := DeriveKitKey(testSecret(99), 1, "resume")
	if _, err := SignControl(request, privates[0], AuthorityKey{KeyID: KeyID(other), PublicKeyHex: hex.EncodeToString(other)}); !errors.Is(err, ErrControl) {
		t.Fatal("signed for a key outside the authority", err)
	}
}

func TestControlRequestMustAgreeWithItsArtifact(t *testing.T) {
	for name, change := range map[string]func(*ControlRequest){
		"schema":    func(r *ControlRequest) { r.Schema = "dytallix.control-request.v0" },
		"kind":      func(r *ControlRequest) { r.Kind = "dytallix-upgrade-control-v2" },
		"digest":    func(r *ControlRequest) { r.Envelope.ArtifactSHA512 = strings.Repeat("0", 128) },
		"sequence":  func(r *ControlRequest) { r.Envelope.Sequence = 5 },
		"window":    func(r *ControlRequest) { r.Envelope.NotAfterHeight = 34661 },
		"action":    func(r *ControlRequest) { r.Envelope.Action = "upgrade" },
		"chain":     func(r *ControlRequest) { r.Envelope.ChainID = "dytallix-mainnet-1" },
		"operation": func(r *ControlRequest) { r.Operation = "resume" },
		"anchor":    func(r *ControlRequest) { r.AnchorHeight = 99 },
		"artifact": func(r *ControlRequest) {
			raw, _ := hex.DecodeString(r.ArtifactHex)
			raw = bytes.Replace(raw, []byte(`"sequence":4`), []byte(`"sequence":5`), 1)
			digest := sha512.Sum512(raw)
			r.ArtifactHex, r.Envelope.ArtifactSHA512 = hex.EncodeToString(raw), hex.EncodeToString(digest[:])
		},
	} {
		request, _, _ := testControl(t)
		change(&request)
		if _, _, err := request.Check(); !errors.Is(err, ErrControl) {
			t.Fatalf("%s: accepted", name)
		}
	}
}

// A request written by the node's dytallix-control (a freeze on the
// development rehearsal) reads back the same in the signer.
func TestControlRequestFromTheNode(t *testing.T) {
	raw, err := os.ReadFile("testdata/control-request-freeze.json")
	if err != nil {
		t.Fatal(err)
	}
	var request ControlRequest
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		t.Fatal(err)
	}
	envelope, summary, err := request.Check()
	if err != nil {
		t.Fatal(err)
	}
	if summary.Operation != "freeze" || summary.ChainID != "dytallix-rehearsal-1" || summary.Sequence != 1 ||
		summary.AnchorHeight != 120 || summary.NotBefore != 121 || envelope.Action != Emergency || len(request.Authority.Keys) != 5 {
		t.Fatalf("unexpected summary %+v", summary)
	}
}

// A kit replacement request written by the node's dytallix-control (root kit
// replacement v1): the signer shows the leaving and the new key IDs, the new
// keys must be listed so that they can sign their proofs, and both a current
// upgrade key and a new key sign it.
func TestKitReplacementRequest(t *testing.T) {
	raw, err := os.ReadFile("testdata/control-request-kit-replacement.json")
	if err != nil {
		t.Fatal(err)
	}
	var request ControlRequest
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		t.Fatal(err)
	}
	envelope, summary, err := request.Check()
	if err != nil {
		t.Fatal(err)
	}
	var payload struct {
		Keys map[string]AuthorityKey `json:"keys"`
	}
	if err := json.Unmarshal(request.Payload, &payload); err != nil {
		t.Fatal(err)
	}
	rendered := summary.Render()
	if summary.Operation != "kit-replacement" || summary.Sequence != 2 || envelope.Action != Upgrade ||
		!strings.Contains(rendered, "new_freeze       "+payload.Keys["freeze"].KeyID) ||
		!strings.Contains(rendered, "authority_epoch  1") {
		t.Fatal(rendered)
	}
	unlisted := request
	unlisted.Authority.Keys = request.Authority.Keys[:len(request.Authority.Keys)-1]
	if _, _, err := unlisted.Check(); !errors.Is(err, ErrControl) {
		t.Fatal("a request that does not list a new key was accepted")
	}

	// Signing: a current upgrade key and a new key, over a request built
	// from test kits.
	var current []AuthorityKey
	var privates [][]byte
	for kit := 1; kit <= KitCount; kit++ {
		public, private, err := DeriveKitKey(testSecret(byte(60+kit)), kit, "upgrade")
		if err != nil {
			t.Fatal(err)
		}
		current = append(current, AuthorityKey{KeyID: KeyID(public), PublicKeyHex: hex.EncodeToString(public)})
		privates = append(privates, private)
	}
	newKeys := map[string]AuthorityKey{}
	var newPrivate []byte
	for _, role := range replacementRoles {
		public, private, err := DeriveKitKey(testSecret(77), 5, role)
		if err != nil {
			t.Fatal(err)
		}
		newKeys[role] = AuthorityKey{KeyID: KeyID(public), PublicKeyHex: hex.EncodeToString(public)}
		if role == "freeze" {
			newPrivate = private
		}
	}
	key := func(k AuthorityKey) string {
		return `{"key_id":"` + k.KeyID + `","public_key_hex":"` + k.PublicKeyHex + `"}`
	}
	payloadJSON := `{"schema":1,"chain_id":"dytallix-staging-1","genesis_sha256":"` + strings.Repeat("c", 64) +
		`","policy_sha256":"` + strings.Repeat("d", 64) + `","authority_epoch":1,"sequence":2,"anchor_height":100,` +
		`"anchor_app_hash":"` + strings.Repeat("b", 64) + `","not_before_height":101,"not_after_height":200,` +
		`"replaced":{"upgrade":"` + current[4].KeyID + `","freeze":"` + strings.Repeat("1", 64) + `","resume":"` +
		strings.Repeat("2", 64) + `"},"keys":{"upgrade":` + key(newKeys["upgrade"]) + `,"freeze":` + key(newKeys["freeze"]) +
		`,"resume":` + key(newKeys["resume"]) + `}}`
	artifact := append([]byte("DYTALLIX/ROOT-KIT-REPLACEMENT/v1\x00"), payloadJSON...)
	digest := sha512.Sum512(artifact)
	built := ControlRequest{
		Schema: ControlRequestSchema, Operation: "kit-replacement", Kind: ReplacementKind,
		AnchorHeight: 100, AnchorAppHash: strings.Repeat("b", 64), Payload: json.RawMessage(payloadJSON),
		ArtifactHex: hex.EncodeToString(artifact),
		Envelope: ControlEnvelope{ChainID: "dytallix-staging-1", Action: "upgrade", Sequence: 2,
			NotBeforeHeight: 101, NotAfterHeight: 200, ArtifactSHA512: hex.EncodeToString(digest[:])},
		Authority: ControlAuthority{Purpose: "upgrade", Threshold: 3, MaxSignatures: 3,
			Keys: append(append([]AuthorityKey{}, current...), newKeys["upgrade"], newKeys["freeze"], newKeys["resume"])},
	}
	for _, signer := range []struct {
		private []byte
		key     AuthorityKey
	}{{privates[0], current[0]}, {newPrivate, newKeys["freeze"]}} {
		signature, err := SignControl(built, signer.private, signer.key)
		if err != nil {
			t.Fatal(err)
		}
		if err := VerifyControlSignature(built, signature, signer.key); err != nil {
			t.Fatal(err)
		}
	}
}

// A restart request written by the node's dytallix-state-check (restart
// v1; the vector is checked by the node's own test): the upgrade keys sign
// it under the upgrade action for the halted height alone, and the signer
// shows the releases, the bindings and whether block H was decided.
func TestRestartRequestFromTheNode(t *testing.T) {
	raw, err := os.ReadFile("testdata/control-request-restart.json")
	if err != nil {
		t.Fatal(err)
	}
	var request ControlRequest
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		t.Fatal(err)
	}
	envelope, summary, err := request.Check()
	if err != nil {
		t.Fatal(err)
	}
	rendered := summary.Render()
	if summary.Operation != "restart" || summary.ChainID != "dytallix-rehearsal-1" || summary.Sequence != 2 ||
		summary.AnchorHeight != 120 || summary.NotBefore != 121 || summary.NotAfter != 121 ||
		envelope.Action != Upgrade || envelope.NotBeforeHeight != 121 || envelope.NotAfterHeight != 121 ||
		!strings.Contains(rendered, "target_release_sha512 "+strings.Repeat("4", 128)) ||
		!strings.Contains(rendered, "halted_block_hash "+strings.Repeat("5", 64)) ||
		!strings.Contains(rendered, "evidence_sha256  "+strings.Repeat("2", 64)) {
		t.Fatalf("unexpected summary %+v\n%s", summary, rendered)
	}

	// The payload rewritten, with the envelope's digest made to agree.
	rewrite := func(r *ControlRequest, from, to string) {
		artifact, _ := hex.DecodeString(r.ArtifactHex)
		if !bytes.Contains(artifact, []byte(from)) {
			t.Fatalf("the artifact has no %s", from)
		}
		artifact = bytes.Replace(artifact, []byte(from), []byte(to), 1)
		digest := sha512.Sum512(artifact)
		r.ArtifactHex, r.Envelope.ArtifactSHA512 = hex.EncodeToString(artifact), hex.EncodeToString(digest[:])
	}
	undecided := request
	rewrite(&undecided, `"halted_block_hash":"`+strings.Repeat("5", 64)+`"`, `"halted_block_hash":null`)
	if _, summary, err := undecided.Check(); err != nil ||
		!strings.Contains(summary.Render(), "halted_block_hash none: block H was never decided") {
		t.Fatal("a restart for an undecided block H", err, summary.Render())
	}
	for name, change := range map[string]func(*ControlRequest){
		// The halted height is the anchor's next block.
		"halted height": func(r *ControlRequest) {
			rewrite(r, `"halted_height":121`, `"halted_height":123`)
			r.Envelope.NotBeforeHeight, r.Envelope.NotAfterHeight = 123, 123
		},
		"window":    func(r *ControlRequest) { r.Envelope.NotAfterHeight = 122 },
		"action":    func(r *ControlRequest) { r.Envelope.Action = "emergency" },
		"operation": func(r *ControlRequest) { r.Operation = "handover-activate" },
		"anchor":    func(r *ControlRequest) { r.AnchorHeight = 121 },
		"kind":      func(r *ControlRequest) { r.Kind = "dytallix-release-handover-v2" },
	} {
		changed := request
		change(&changed)
		if _, _, err := changed.Check(); !errors.Is(err, ErrControl) {
			t.Fatalf("%s: accepted", name)
		}
	}

	// Signed with test kits' upgrade keys listed as the authority.
	var keys []AuthorityKey
	var privates [][]byte
	for kit := 1; kit <= KitCount; kit++ {
		public, private, err := DeriveKitKey(testSecret(byte(80+kit)), kit, "upgrade")
		if err != nil {
			t.Fatal(err)
		}
		keys = append(keys, AuthorityKey{KeyID: KeyID(public), PublicKeyHex: hex.EncodeToString(public)})
		privates = append(privates, private)
	}
	signing := request
	signing.Authority.Keys = keys
	signature, err := SignControl(signing, privates[2], keys[2])
	if err != nil {
		t.Fatal(err)
	}
	if signature.Sequence != 2 || signature.ArtifactSHA512 != request.Envelope.ArtifactSHA512 {
		t.Fatalf("unexpected signature record %+v", signature)
	}
	if err := VerifyControlSignature(signing, signature, keys[2]); err != nil {
		t.Fatal(err)
	}
	// As the node's helper checks it: at the halted height, under the
	// upgrade action.
	signatureBytes, _ := hex.DecodeString(signature.SignatureHex)
	public, _ := hex.DecodeString(keys[2].PublicKeyHex)
	if err := Verify(envelope, signatureBytes, Policy{TrustedPublicKey: public, ChainID: envelope.ChainID,
		Action: Upgrade, ExpectedSequence: 2, CurrentHeight: 121, ExpectedArtifactDigest: envelope.ArtifactDigest}); err != nil {
		t.Fatal(err)
	}
}
