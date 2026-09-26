package rootauthorization

import (
	"bytes"
	"crypto/sha512"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestVerificationWire(t *testing.T) {
	pub, priv := localPair(t)
	e, p := localRequest(pub)
	signature, err := SignForPolicy(e, priv, p)
	if err != nil {
		t.Fatal(err)
	}
	request := VerificationRequest{Envelope: e, Signature: signature, Artifact: []byte("local fixture only")}
	raw, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	result, err := VerifyRequest(p, raw)
	if err != nil || result.Status != "VERIFIED" || result.ProductionQualified {
		t.Fatal(err)
	}
	for _, data := range [][]byte{append(bytes.Clone(raw), '\n'), append([]byte("{\"Extra\":1,"), raw[1:]...), append([]byte("{\"Envelope\":{},"), raw[1:]...)} {
		if _, err := VerifyRequest(p, data); err == nil {
			t.Fatal("noncanonical request accepted")
		}
	}
	p.ExpectedArtifactDigest[0] ^= 1
	if _, err := VerifyRequest(p, raw); err == nil {
		t.Fatal("untrusted artifact accepted")
	}
}

// TestExportDevelopmentGenesis creates public fixture material only when an
// explicit output directory is supplied. Private signing keys stay in memory.
func TestExportDevelopmentGenesis(t *testing.T) {
	directory := os.Getenv("DYT_ROOT_PUBLIC_FIXTURE_DIR")
	if directory == "" {
		t.Skip("explicit public fixture export not requested")
	}
	app, err := os.ReadFile(filepath.Join(directory, "genesis.json"))
	if err != nil {
		t.Fatal(err)
	}
	config, err := os.ReadFile(filepath.Join(directory, "consensus.json"))
	if err != nil {
		t.Fatal(err)
	}
	var metadata struct {
		ChainID string `json:"chain_id"`
	}
	if err := json.Unmarshal(config, &metadata); err != nil || !validChainID(metadata.ChainID) {
		t.Fatal("invalid fixture chain")
	}
	pub, priv := localPair(t)
	// Tests bind exact development fixture files. These are not release data.
	engine, err := os.ReadFile(filepath.Join(directory, "engine-genesis.json"))
	if err != nil || len(engine) == 0 {
		t.Fatal("missing or empty engine genesis fixture", err)
	}
	release, err := os.ReadFile(filepath.Join(directory, "release-manifest.json"))
	if err != nil || len(release) == 0 {
		t.Fatal("missing or empty release manifest fixture", err)
	}
	artifact := genesisBundleForTest(app, config, sha512.Sum512(engine), sha512.Sum512(release))
	e := Envelope{Version: Version, Profile: Profile, ChainID: metadata.ChainID, Action: Genesis, Sequence: 1, NotBeforeHeight: 0, NotAfterHeight: 0, ArtifactDigest: DigestArtifact(artifact)}
	p := Policy{TrustedPublicKey: pub, ChainID: e.ChainID, Action: Genesis, ExpectedSequence: 1, CurrentHeight: 0, ExpectedArtifactDigest: e.ArtifactDigest}
	sig, err := SignForPolicy(e, priv, p)
	if err != nil {
		t.Fatal(err)
	}
	for name, value := range map[string]any{"policy.json": p, "request.json": VerificationRequest{Envelope: e, Signature: sig, Artifact: artifact}} {
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		f, err := os.OpenFile(filepath.Join(directory, name), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = f.Write(encoded); err != nil {
			f.Close()
			t.Fatal(err)
		}
		if err = f.Close(); err != nil {
			t.Fatal(err)
		}
	}
}
