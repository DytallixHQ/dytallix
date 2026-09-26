// Command dytallix-root-verify verifies one request. It cannot sign, deploy,
// initialize chain state, consume replay sequences or grant launch acceptance.
package main

import (
	ownerguard "dytallix.local/consensus/owner-guard"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	root "github.com/dytallix/root-authorization"
)

var errVerificationRejected = errors.New("root verification rejected")

const (
	executionProfile = "linux-immutable-observed-helper-v1"
	readyToken       = "DYTALLIX-ROOT-READY-v1\n"
	ackToken         = "DYTALLIX-ROOT-ACK-v1\n"
	maxResultBytes   = 4096
)

func exitCode(err error) int {
	if err == nil {
		return 0
	}
	if errors.Is(err, errVerificationRejected) {
		return 2
	}
	return 1
}

func run(args []string, input io.Reader, output io.Writer) error {
	flags := flag.NewFlagSet("dytallix-root-verify", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	policyJSON := flags.String("policy-json", "", "independently trusted public policy; never derived from request")
	limit := flags.Int64("max-input-bytes", 0, "explicit request limit")
	profile := flags.String("profile", "", "explicit cryptographic profile")
	execution := flags.String("execution-profile", "", "explicit observed execution protocol")
	if err := flags.Parse(args); err != nil {
		return errors.New("invalid verification arguments")
	}
	if flags.NArg() != 0 || *limit <= 0 || *limit > int64(^uint32(0)) || *policyJSON == "" || *profile != root.Profile || *execution != executionProfile {
		return errors.New("explicit trusted policy, cryptographic and execution profiles, and size limit required")
	}
	// Fixed policy fields contain one 64-byte public key and bounded identifiers.
	// This parser guard is a helper format bound, not a production request policy.
	if len(*policyJSON) > 16384 {
		return errors.New("policy exceeds helper format bound")
	}
	var policy root.Policy
	if err := root.DecodeCanonical([]byte(*policyJSON), &policy); err != nil {
		return errors.New("invalid trusted policy encoding")
	}
	// The caller observes this blocked process before it releases request bytes.
	if err := writeExact(output, []byte(readyToken)); err != nil {
		return errors.New("cannot write helper readiness")
	}
	var header [4]byte
	if _, err := io.ReadFull(input, header[:]); err != nil {
		return errors.New("cannot read verification request frame")
	}
	length := binary.BigEndian.Uint32(header[:])
	if length == 0 || int64(length) > *limit || uint64(length) > uint64(^uint(0)>>1) {
		return errors.New("verification request frame exceeds explicit limit")
	}
	raw := make([]byte, int(length))
	if _, err := io.ReadFull(input, raw); err != nil {
		return errors.New("cannot read complete verification request")
	}
	result, err := root.VerifyRequest(policy, raw)
	status := byte(0)
	var encoded []byte
	if err != nil {
		status = 2
	} else {
		encoded, err = jsonBytes(result)
		if err != nil {
			return err
		}
	}
	if err := writeOutcome(output, status, encoded); err != nil {
		return err
	}
	// Keep the same helper alive for the caller's post-verification observation.
	ack := make([]byte, len(ackToken))
	if _, err := io.ReadFull(input, ack); err != nil || string(ack) != ackToken {
		return errors.New("invalid or incomplete observation acknowledgement")
	}
	var trailing [1]byte
	if count, err := io.ReadFull(input, trailing[:]); count != 0 || err != io.EOF {
		return errors.New("observation acknowledgement requires EOF")
	}
	if status == 2 {
		return errVerificationRejected
	}
	return nil
}

func writeExact(output io.Writer, raw []byte) error {
	count, err := output.Write(raw)
	if err != nil {
		return err
	}
	if count != len(raw) {
		return io.ErrShortWrite
	}
	return nil
}

func writeOutcome(output io.Writer, status byte, raw []byte) error {
	if (status == 0 && (len(raw) == 0 || len(raw) > maxResultBytes)) || (status == 2 && len(raw) != 0) || (status != 0 && status != 2) {
		return errors.New("invalid verification result frame")
	}
	var header [5]byte
	header[0] = status
	binary.BigEndian.PutUint32(header[1:], uint32(len(raw)))
	if err := writeExact(output, header[:]); err != nil {
		return errors.New("cannot write verification result frame")
	}
	if len(raw) > 0 {
		if err := writeExact(output, raw); err != nil {
			return errors.New("cannot write verification result payload")
		}
	}
	return nil
}
func main() {
	if err := ownerguard.Run(ownerguard.Helper, func() error { return run(os.Args[1:], os.Stdin, os.Stdout) }); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(exitCode(err))
	}
}
