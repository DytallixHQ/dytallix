package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strconv"

	root "dytallix.local/consensus/root-authorization"
)

func readRequest(path string) (root.ControlRequest, error) {
	var request root.ControlRequest
	raw, err := readBounded(path, maxRecordBytes)
	if err != nil {
		return request, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.More() {
		return request, fmt.Errorf("%s is not a control request", path)
	}
	return request, nil
}

func showControl(path string, out io.Writer) error {
	request, err := readRequest(path)
	if err != nil {
		return err
	}
	_, summary, err := request.Check()
	if err != nil {
		return err
	}
	if request.Kind == root.ReplacementKind {
		// The request lists the current upgrade keys and the three new keys.
		_, err = fmt.Fprintf(out, "%ssigners          %d of the %d current upgrade keys, and each of the 3 new keys\n",
			summary.Render(), request.Authority.Threshold, len(request.Authority.Keys)-3)
		return err
	}
	_, err = fmt.Fprintf(out, "%ssigners          %d of the %d %s keys\n", summary.Render(),
		request.Authority.Threshold, len(request.Authority.Keys), request.Authority.Purpose)
	return err
}

// signControl signs only the operation and sequence the custodian typed,
// after reading them from the artifact.
func signControl(requestPath, keyPath, publicPath, operation, sequence, output string, out io.Writer) error {
	request, err := readRequest(requestPath)
	if err != nil {
		return err
	}
	_, summary, err := request.Check()
	if err != nil {
		return err
	}
	if operation != summary.Operation || sequence != strconv.FormatUint(summary.Sequence, 10) {
		return fmt.Errorf("this request is %s sequence %d, not %s sequence %s; run show-control and check it",
			summary.Operation, summary.Sequence, operation, sequence)
	}
	var key root.AuthorityKey
	if err := readRecord(publicPath, &key); err != nil {
		return err
	}
	private, err := readPrivateKey(keyPath)
	if err != nil {
		return err
	}
	defer clear(private)
	signature, err := root.SignControl(request, private, key)
	if err != nil {
		if errors.Is(err, root.ErrKey) || errors.Is(err, root.ErrPolicy) {
			return errors.New("the private key does not match its public key record")
		}
		return err
	}
	if _, err := fmt.Fprint(out, summary.Render()); err != nil {
		return err
	}
	return writeRecord(output, signature, out)
}

func verifyControl(requestPath, signaturePath, publicPath string, out io.Writer) error {
	request, err := readRequest(requestPath)
	if err != nil {
		return err
	}
	var key root.AuthorityKey
	if err := readRecord(publicPath, &key); err != nil {
		return err
	}
	raw, err := readBounded(signaturePath, maxRecordBytes)
	if err != nil {
		return err
	}
	var signature root.ControlSignature
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&signature); err != nil {
		return fmt.Errorf("%s is not a control signature", signaturePath)
	}
	if err := root.VerifyControlSignature(request, signature, key); err != nil {
		return err
	}
	_, err = fmt.Fprintf(out, "verified %s sequence %d key_id %s\n", request.Operation, signature.Sequence, key.KeyID)
	return err
}
