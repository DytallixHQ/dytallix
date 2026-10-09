package main

import (
	"bytes"
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	root "dytallix.local/consensus/root-authorization"
)

// Signing a freeze offline with kit keys, as a custodian does: show it,
// refuse the wrong confirmation, sign, and verify the signature file.
func TestSignControlWithKitKeys(t *testing.T) {
	dir := t.TempDir()
	public := filepath.Join(dir, "public")
	if err := os.Mkdir(public, 0o755); err != nil {
		t.Fatal(err)
	}
	var keys []root.AuthorityKey
	for n := 1; n <= 5; n++ {
		drive := filepath.Join(dir, fmt.Sprintf("kit-%d", n))
		if err := os.Mkdir(drive, 0o700); err != nil {
			t.Fatal(err)
		}
		runOK(t, "kit", "-number", fmt.Sprint(n), "-private-out", drive, "-public-out", public)
		var key root.AuthorityKey
		if err := readRecord(filepath.Join(public, kitName(n, "freeze", ".json")), &key); err != nil {
			t.Fatal(err)
		}
		keys = append(keys, key)
	}
	payload := `{"schema":2,"chain_id":"dytallix-staging-1","release_sha512":"` + strings.Repeat("a", 128) +
		`","action":"freeze","sequence":7,"parent_height":500,"parent_app_hash":"` + strings.Repeat("b", 64) +
		`","target_height":501,"v2":{"genesis_sha256":"` + strings.Repeat("c", 64) + `","authority_epoch":1,"policy_sha256":"` +
		strings.Repeat("d", 64) + `","not_before_height":501,"not_after_height":35060,"incident_sha256":"` + strings.Repeat("e", 64) + `"}}`
	artifact := append([]byte("DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v2\x00"), payload...)
	digest := sha512.Sum512(artifact)
	request := root.ControlRequest{
		Schema: root.ControlRequestSchema, Operation: "freeze", Kind: "dytallix-emergency-control-v2",
		AnchorHeight: 500, AnchorAppHash: strings.Repeat("b", 64), Payload: json.RawMessage(payload),
		ArtifactHex: hex.EncodeToString(artifact),
		Envelope: root.ControlEnvelope{ChainID: "dytallix-staging-1", Action: "emergency", Sequence: 7,
			NotBeforeHeight: 501, NotAfterHeight: 35060, ArtifactSHA512: hex.EncodeToString(digest[:])},
		Authority: root.ControlAuthority{Purpose: "freeze", Threshold: 3, MaxSignatures: 3, Keys: keys},
	}
	raw, _ := json.MarshalIndent(request, "", "  ")
	requestPath := filepath.Join(dir, "request.json")
	if err := os.WriteFile(requestPath, raw, 0o644); err != nil {
		t.Fatal(err)
	}
	if out := runOK(t, "show-control", "-request", requestPath); !strings.Contains(out, "operation        freeze") ||
		!strings.Contains(out, "window           501 to 35060") || !strings.Contains(out, "3 of the 5 freeze keys") {
		t.Fatal(out)
	}
	sign := func(operation, sequence, out string) error {
		return run([]string{"sign-control", "-request", requestPath, "-private-key", filepath.Join(dir, "kit-2", kitName(2, "freeze", ".key")),
			"-public-key", filepath.Join(public, kitName(2, "freeze", ".json")), "-operation", operation, "-sequence", sequence,
			"-out", out}, &bytes.Buffer{})
	}
	// The custodian types what they expect; a mismatch signs nothing.
	for _, c := range [][2]string{{"resume", "7"}, {"freeze", "8"}} {
		if err := sign(c[0], c[1], filepath.Join(dir, "never.json")); err == nil {
			t.Fatalf("signed %v", c)
		}
	}
	signature := filepath.Join(dir, "kit-2-signature.json")
	if err := sign("freeze", "7", signature); err != nil {
		t.Fatal(err)
	}
	out := runOK(t, "verify-control", "-request", requestPath, "-signature", signature,
		"-public-key", filepath.Join(public, kitName(2, "freeze", ".json")))
	if !strings.Contains(out, "verified freeze sequence 7") {
		t.Fatal(out)
	}
	if err := run([]string{"verify-control", "-request", requestPath, "-signature", signature,
		"-public-key", filepath.Join(public, kitName(3, "freeze", ".json"))}, &bytes.Buffer{}); err == nil {
		t.Fatal("a signature verified under another kit's key")
	}
	// A kit's resume key is not a freeze key.
	if err := run([]string{"sign-control", "-request", requestPath, "-private-key", filepath.Join(dir, "kit-2", kitName(2, "resume", ".key")),
		"-public-key", filepath.Join(public, kitName(2, "resume", ".json")), "-operation", "freeze", "-sequence", "7",
		"-out", filepath.Join(dir, "resume.json")}, &bytes.Buffer{}); err == nil {
		t.Fatal("a resume key signed a freeze")
	}
}

// A kit replacement request shows the seat and the new keys, and who signs.
func TestShowKitReplacement(t *testing.T) {
	out := runOK(t, "show-control", "-request", filepath.Join("..", "..", "testdata", "control-request-kit-replacement.json"))
	for _, want := range []string{"operation        kit-replacement", "leaving_upgrade", "new_resume",
		"3 of the 5 current upgrade keys, and each of the 3 new keys"} {
		if !strings.Contains(out, want) {
			t.Fatal(out)
		}
	}
}
