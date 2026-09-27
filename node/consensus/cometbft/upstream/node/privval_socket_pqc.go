//go:build dytallix_pqc_only

package node

import (
	"errors"

	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/types"
)

// Remote signing is not compiled into PQC-only builds. Validators sign with the
// local file key, which the engine loads from checked bytes.
func createAndStartPrivValidatorSocketClient(
	string,
	string,
	*p2p.NodeKey,
	log.Logger,
) (types.PrivValidator, error) {
	return nil, errors.New("remote signing excluded by dytallix_pqc_only")
}
