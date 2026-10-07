package rootauthorization

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"strings"
)

// A code line is a 32-byte secret written on paper: a prefix naming what it
// opens and its label, the secret in sixteen groups of four hex digits, and
// a two-byte check group over the label and the secret. The seal codes and
// the backup codes use it, each under its own prefix and check domain.
const codeLineCheckBytes = 2

func codeLineCheck(domain, label string, code []byte) []byte {
	sum := make([]byte, codeLineCheckBytes)
	shake(sum, []byte(domain), []byte{byte(len(label))}, []byte(label), code)
	return sum
}

func encodeCodeLine(prefix, domain, label string, code []byte) string {
	digits := hex.EncodeToString(append(append([]byte{}, code...), codeLineCheck(domain, label, code)...))
	groups := make([]string, 0, len(digits)/4)
	for i := 0; i < len(digits); i += 4 {
		groups = append(groups, digits[i:i+4])
	}
	return fmt.Sprintf("%s%s %s", prefix, label, strings.Join(groups, " "))
}

// decodeCodeLine reads a line back, ignoring spacing and the case of the
// digits. A mistyped digit or label fails the check group (one in 65,536
// errors would pass it; the authenticated encryption then catches the rest).
// valid checks the label; err is the error to wrap.
func decodeCodeLine(line, prefix, domain string, codeBytes int, valid func(string) bool, err error, what string) (string, []byte, error) {
	fields := strings.Fields(line)
	if len(fields) < 2 || !strings.HasPrefix(strings.ToLower(fields[0]), prefix) {
		return "", nil, err
	}
	label := fields[0][len(prefix):]
	if !valid(label) {
		return "", nil, err
	}
	raw, decodeErr := hex.DecodeString(strings.ToLower(strings.Join(fields[1:], "")))
	if decodeErr != nil || len(raw) != codeBytes+codeLineCheckBytes {
		clear(raw)
		return "", nil, err
	}
	code := raw[:codeBytes]
	if !bytes.Equal(codeLineCheck(domain, label, code), raw[codeBytes:]) {
		clear(raw)
		return "", nil, fmt.Errorf("%w: the check group does not match; recheck every digit and the %s", err, what)
	}
	return label, code, nil
}
