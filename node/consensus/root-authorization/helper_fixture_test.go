package rootauthorization

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// TestExportHelperQualificationFixture writes a public policy, a valid signed
// request and the same request with one changed signature byte, for native
// helper runs (E02). It runs only when DYT_E02_FIXTURE_DIR is set. The signing
// key is generated in memory and never written.
func TestExportHelperQualificationFixture(t *testing.T) {
	directory := os.Getenv("DYT_E02_FIXTURE_DIR")
	if directory == "" {
		t.Skip("explicit helper fixture export not requested")
	}
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	signature, err := SignForPolicy(e, priv, p)
	if err != nil {
		t.Fatal(err)
	}
	artifact := []byte("local fixture only")
	good := VerificationRequest{Envelope: e, Signature: signature, Artifact: artifact}
	changed := append([]byte(nil), signature...)
	changed[len(changed)/2] ^= 1
	bad := VerificationRequest{Envelope: e, Signature: changed, Artifact: artifact}
	for name, value := range map[string]any{"policy.json": p, "request.json": good, "bad-request.json": bad} {
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, name), encoded, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	raw, _ := json.Marshal(good)
	if result, err := VerifyRequest(p, raw); err != nil || result.Status != "VERIFIED" {
		t.Fatal("exported valid request does not verify", err)
	}
	raw, _ = json.Marshal(bad)
	if _, err := VerifyRequest(p, raw); err == nil {
		t.Fatal("exported changed request verifies")
	}
}
