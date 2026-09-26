package main

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"testing"
	"time"

	"github.com/cloudflare/circl/sign/slhdsa"
	root "github.com/dytallix/root-authorization"
)

func helperArgs(policy []byte, limit string) []string {
	return []string{"--profile", root.Profile, "--execution-profile", executionProfile, "--policy-json", string(policy), "--max-input-bytes", limit}
}
func framedRequest(raw []byte, tail []byte) []byte {
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(len(raw)))
	result := append(header[:], raw...)
	return append(result, tail...)
}
func readResult(t *testing.T, raw []byte) (byte, []byte) {
	t.Helper()
	if !bytes.HasPrefix(raw, []byte(readyToken)) {
		t.Fatal("exact readiness token missing")
	}
	raw = raw[len(readyToken):]
	if len(raw) < 5 {
		t.Fatal("result header missing")
	}
	length := binary.BigEndian.Uint32(raw[1:5])
	if uint64(length) != uint64(len(raw)-5) {
		t.Fatal("result length differs")
	}
	return raw[0], raw[5:]
}
func TestHelperRejectsUntrustedSetup(t *testing.T) {
	policy, _ := json.Marshal(root.Policy{})
	good := helperArgs(policy, "4096")
	tests := [][]string{nil,
		{"--policy-json", string(policy), "--max-input-bytes", "1024", "--profile", root.Profile},
		helperArgs(policy, "0"), helperArgs(policy, "4294967296"), helperArgs([]byte("{}"), "1024"),
		append(append([]string{}, good...), "unexpected"),
	}
	wrong := append([]string{}, good...)
	wrong[3] = "legacy"
	tests = append(tests, wrong)
	for i, args := range tests {
		var output bytes.Buffer
		input := &countedInput{Reader: bytes.NewReader(framedRequest([]byte("{}"), []byte(ackToken)))}
		if err := run(args, input, &output); err == nil || output.Len() != 0 || input.reads != 0 {
			t.Fatalf("invalid setup %d accepted or consumed input", i)
		}
	}
}

type countedInput struct {
	io.Reader
	reads int
}

func (r *countedInput) Read(p []byte) (int, error) { r.reads++; return r.Reader.Read(p) }

type failedInput struct{}

func (failedInput) Read([]byte) (int, error) { return 0, io.ErrClosedPipe }

type failedOutput struct{}

func (failedOutput) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

type shortOutput struct{}

func (shortOutput) Write(p []byte) (int, error) { return len(p) - 1, nil }

type failAfterReady struct{ writes int }

func (w *failAfterReady) Write(p []byte) (int, error) {
	w.writes++
	if w.writes > 1 {
		return 0, io.ErrClosedPipe
	}
	return len(p), nil
}

func TestExitCodesSeparateRefusalFromInfrastructure(t *testing.T) {
	if exitCode(nil) != 0 || exitCode(errVerificationRejected) != 2 || exitCode(io.ErrClosedPipe) != 1 || exitCode(fmt.Errorf("context: %w", errVerificationRejected)) != 2 {
		t.Fatal("exit classification differs")
	}
	policy, _ := json.Marshal(root.Policy{})
	args := helperArgs(policy, "4096")
	var output bytes.Buffer
	err := run(args, bytes.NewReader(framedRequest([]byte("{}"), []byte(ackToken))), &output)
	status, payload := readResult(t, output.Bytes())
	if !errors.Is(err, errVerificationRejected) || status != 2 || len(payload) != 0 {
		t.Fatal("complete refusal lacks dedicated exit")
	}
	if err := run(args, failedInput{}, &bytes.Buffer{}); err == nil || exitCode(err) != 1 {
		t.Fatal("input failure classified as refusal")
	}
}

func TestReadinessPrecedesRequestRead(t *testing.T) {
	policy, _ := json.Marshal(root.Policy{})
	input := &countedInput{Reader: bytes.NewReader(framedRequest([]byte("{}"), []byte(ackToken)))}
	err := run(helperArgs(policy, "4096"), input, failedOutput{})
	if exitCode(err) != 1 || input.reads != 0 {
		t.Fatal("request consumed before successful READY write")
	}
	input = &countedInput{Reader: bytes.NewReader(nil)}
	if err := run(helperArgs(policy, "4096"), input, shortOutput{}); exitCode(err) != 1 || input.reads != 0 {
		t.Fatal("short READY write accepted")
	}
}

func TestResultWaitsForAckAndEOF(t *testing.T) {
	policy, _ := json.Marshal(root.Policy{})
	input, parentInput := io.Pipe()
	parentOutput, output := io.Pipe()
	defer input.Close()
	defer parentInput.Close()
	defer parentOutput.Close()
	defer output.Close()
	done := make(chan error, 1)
	go func() { err := run(helperArgs(policy, "4096"), input, output); output.Close(); done <- err }()
	ready := make([]byte, len(readyToken))
	if _, err := io.ReadFull(parentOutput, ready); err != nil || string(ready) != readyToken {
		t.Fatal("READY missing", err)
	}
	select {
	case err := <-done:
		t.Fatal("helper exited before request", err)
	default:
	}
	if _, err := parentInput.Write(framedRequest([]byte("{}"), nil)); err != nil {
		t.Fatal(err)
	}
	header := make([]byte, 5)
	if _, err := io.ReadFull(parentOutput, header); err != nil || !bytes.Equal(header, []byte{2, 0, 0, 0, 0}) {
		t.Fatal("refusal frame differs", err)
	}
	select {
	case err := <-done:
		t.Fatal("helper exited before ACK", err)
	default:
	}
	if _, err := io.WriteString(parentInput, ackToken); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		t.Fatal("helper exited before EOF", err)
	default:
	}
	parentInput.Close()
	select {
	case err := <-done:
		if exitCode(err) != 2 {
			t.Fatal("acknowledged refusal exit differs", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("helper did not complete after ACK and EOF")
	}
}

func TestInvalidFramingAndAckAreInfrastructureFailures(t *testing.T) {
	policy, _ := json.Marshal(root.Policy{})
	args := helperArgs(policy, "8")
	cases := map[string][]byte{
		"empty": nil, "short header": {0, 0}, "zero length": {0, 0, 0, 0}, "oversized": {0, 0, 0, 9}, "short payload": {0, 0, 0, 2, '{'},
		"missing ack": framedRequest([]byte("{}"), nil), "short ack": framedRequest([]byte("{}"), []byte("DYT")),
		"wrong ack":      framedRequest([]byte("{}"), []byte("DYTALLIX-ROOT-NAK-v1\n")),
		"trailing bytes": framedRequest([]byte("{}"), append([]byte(ackToken), 'x')),
		"old transport":  []byte("{}"),
	}
	for name, raw := range cases {
		t.Run(name, func(t *testing.T) {
			var output bytes.Buffer
			err := run(args, bytes.NewReader(raw), &output)
			if err == nil || exitCode(err) != 1 || errors.Is(err, errVerificationRejected) {
				t.Fatal("bad protocol classified as verification result", err)
			}
		})
	}
}

func TestOutcomeFrameBoundsAndOutputFailures(t *testing.T) {
	for _, tc := range []struct {
		status  byte
		payload []byte
	}{{0, nil}, {0, make([]byte, maxResultBytes+1)}, {2, []byte("x")}, {1, nil}} {
		var output bytes.Buffer
		if err := writeOutcome(&output, tc.status, tc.payload); err == nil || output.Len() != 0 {
			t.Fatal("invalid result frame accepted")
		}
	}
	if err := writeOutcome(shortOutput{}, 2, nil); err == nil {
		t.Fatal("short result header accepted")
	}
}

func TestRealRequestPreservesExactCryptoResponse(t *testing.T) {
	pub, priv, err := slhdsa.GenerateKey(bytes.NewReader(bytes.Repeat([]byte{23}, 96)), slhdsa.SHAKE_256s)
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
	artifact := []byte("development-only-emergency-control")
	envelope := root.Envelope{Version: root.Version, Profile: root.Profile, ChainID: "helper-development", Action: root.Emergency, Sequence: 1, NotBeforeHeight: 10, NotAfterHeight: 10, ArtifactDigest: root.DigestArtifact(artifact)}
	policy := root.Policy{TrustedPublicKey: public, ChainID: envelope.ChainID, Action: root.Emergency, ExpectedSequence: 1, CurrentHeight: 10, ExpectedArtifactDigest: root.DigestArtifact(artifact)}
	signature, err := root.SignForPolicy(envelope, private, policy)
	if err != nil {
		t.Fatal(err)
	}
	request := root.VerificationRequest{Envelope: envelope, Signature: signature, Artifact: artifact}
	raw, _ := json.Marshal(request)
	policyJSON, _ := json.Marshal(policy)
	args := helperArgs(policyJSON, "65536")
	expected, err := root.VerifyRequest(policy, raw)
	if err != nil {
		t.Fatal(err)
	}
	expectedJSON, _ := json.Marshal(expected)
	var output bytes.Buffer
	if err := run(args, bytes.NewReader(framedRequest(raw, []byte(ackToken))), &output); err != nil {
		t.Fatal(err)
	}
	status, payload := readResult(t, output.Bytes())
	if status != 0 || !bytes.Equal(payload, expectedJSON) {
		t.Fatal("cryptographic response bytes changed")
	}
	writer := &failAfterReady{}
	if err := run(args, bytes.NewReader(framedRequest(raw, []byte(ackToken))), writer); exitCode(err) != 1 || writer.writes != 2 {
		t.Fatal("post-verification output failure classified incorrectly")
	}
	request.Signature[100] ^= 1
	raw, _ = json.Marshal(request)
	output.Reset()
	if err := run(args, bytes.NewReader(framedRequest(raw, []byte(ackToken))), &output); exitCode(err) != 2 {
		t.Fatal("signature refusal classification differs", err)
	}
	status, payload = readResult(t, output.Bytes())
	if status != 2 || len(payload) != 0 {
		t.Fatal("signature refusal emitted success payload")
	}
}
