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
