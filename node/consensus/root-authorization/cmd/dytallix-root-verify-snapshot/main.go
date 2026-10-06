// Command dytallix-root-verify-snapshot verifies one request for the node's
// historical snapshot launch (E04 gap 11).
//
// TEST ONLY. The node accepts this launch only in test builds
// (root_genesis::validate_helper_execution); production runs
// dytallix-root-verify under the observed owner protocol. This command
// performs the same root.VerifyRequest but reads the whole request from
// stdin and writes the result to stdout, without the owner guard, so the
// signed migration, handover and emergency tests run on any host.
package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	root "dytallix.local/consensus/root-authorization"
)

var errVerificationRejected = errors.New("root verification rejected")

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
	if err := flags.Parse(args); err != nil {
		return errors.New("invalid verification arguments")
	}
	if flags.NArg() != 0 || *limit <= 0 || *limit == int64(^uint64(0)>>1) || *policyJSON == "" || *profile != root.Profile {
		return errors.New("explicit trusted policy, profile and size limit required")
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
	raw, err := io.ReadAll(io.LimitReader(input, *limit+1))
	if err != nil {
		return errors.New("cannot read verification request")
	}
	if int64(len(raw)) > *limit {
		return errors.New("verification request exceeds explicit limit")
	}
	result, err := root.VerifyRequest(policy, raw)
	if err != nil {
		return errVerificationRejected
	}
	encoded, err := jsonBytes(result)
	if err != nil {
		return err
	}
	_, err = output.Write(encoded)
	return err
}
func main() {
	if err := run(os.Args[1:], os.Stdin, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(exitCode(err))
	}
}

func jsonBytes(value any) ([]byte, error) { return json.Marshal(value) }
