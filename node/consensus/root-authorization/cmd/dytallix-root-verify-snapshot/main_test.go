package main

import (
	"bytes"
	"encoding/json"
	"testing"

	root "dytallix.local/consensus/root-authorization"
	"github.com/cloudflare/circl/sign/slhdsa"
)

func signedRequest(t *testing.T) (root.Policy, root.VerificationRequest) {
	t.Helper()
	pub, priv, err := slhdsa.GenerateKey(bytes.NewReader(bytes.Repeat([]byte{29}, 96)), slhdsa.SHAKE_256s)
	if err != nil {
		t.Fatal(err)
	}
	public, err := pub.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	private, err := priv.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		for i := range private {
			private[i] = 0
		}
	}()
	artifact := []byte("development-only-snapshot-control")
	envelope := root.Envelope{Version: root.Version, Profile: root.Profile, ChainID: "snapshot-development", Action: root.Upgrade, Sequence: 3, NotBeforeHeight: 10, NotAfterHeight: 12, ArtifactDigest: root.DigestArtifact(artifact)}
	policy := root.Policy{TrustedPublicKey: public, ChainID: envelope.ChainID, Action: root.Upgrade, ExpectedSequence: 3, CurrentHeight: 11, ExpectedArtifactDigest: root.DigestArtifact(artifact)}
	signature, err := root.SignForPolicy(envelope, private, policy)
	if err != nil {
		t.Fatal(err)
	}
	return policy, root.VerificationRequest{Envelope: envelope, Signature: signature, Artifact: artifact}
}

func args(t *testing.T, policy root.Policy, limit string) []string {
	t.Helper()
	encoded, err := json.Marshal(policy)
	if err != nil {
		t.Fatal(err)
	}
	return []string{"--profile", root.Profile, "--policy-json", string(encoded), "--max-input-bytes", limit}
}

// The snapshot launch returns root.VerifyRequest's result unchanged and
// refuses a changed signature with exit status 2 and no output.
func TestSnapshotVerifierPreservesVerificationResult(t *testing.T) {
	policy, request := signedRequest(t)
	raw, _ := json.Marshal(request)
	expected, err := root.VerifyRequest(policy, raw)
	if err != nil {
		t.Fatal(err)
	}
	expectedJSON, _ := json.Marshal(expected)
	var output bytes.Buffer
	if err := run(args(t, policy, "65536"), bytes.NewReader(raw), &output); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(output.Bytes(), expectedJSON) {
		t.Fatalf("result %s, expected %s", output.Bytes(), expectedJSON)
	}
	request.Signature[100] ^= 1
	raw, _ = json.Marshal(request)
	output.Reset()
	if err := run(args(t, policy, "65536"), bytes.NewReader(raw), &output); exitCode(err) != 2 || output.Len() != 0 {
		t.Fatalf("changed signature: exit %d, output %q", exitCode(err), output.Bytes())
	}
}

// Setup errors are infrastructure failures (exit 1), never a refusal.
func TestSnapshotVerifierRejectsUntrustedSetup(t *testing.T) {
	policy, request := signedRequest(t)
	raw, _ := json.Marshal(request)
	for name, arguments := range map[string][]string{
		"oversized request": args(t, policy, "16"),
		"missing limit":     {"--profile", root.Profile, "--policy-json", args(t, policy, "1")[3]},
		"wrong profile":     {"--profile", "ML-DSA-65", "--policy-json", args(t, policy, "1")[3], "--max-input-bytes", "65536"},
		"extra argument":    append(args(t, policy, "65536"), "extra"),
		"invalid policy":    {"--profile", root.Profile, "--policy-json", "{", "--max-input-bytes", "65536"},
	} {
		var output bytes.Buffer
		if err := run(arguments, bytes.NewReader(raw), &output); exitCode(err) != 1 || output.Len() != 0 {
			t.Fatalf("%s: exit %d, output %q", name, exitCode(err), output.Bytes())
		}
	}
}
