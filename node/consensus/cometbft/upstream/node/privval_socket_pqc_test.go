package node

import (
	"strings"
	"testing"

	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

// PQC-only builds contain no remote-signer client; any configured listen
// address is rejected before a connection is attempted.
func TestPQCBuildRemoteSignerRejected(t *testing.T) {
	for _, address := range []string{"tcp://127.0.0.1:1", "unix:///unopened.sock", "noise://unused"} {
		signer, err := createAndStartPrivValidatorSocketClient(address, "chain", &p2p.NodeKey{}, log.NewNopLogger())
		if signer != nil || err == nil || !strings.Contains(err.Error(), "not built") {
			t.Fatalf("remote signer accepted for %s: %v", address, err)
		}
	}
}
