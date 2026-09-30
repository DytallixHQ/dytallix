package p2p

import (
	"errors"
	"github.com/cometbft/cometbft/crypto"
	"net"
	"time"
)

// No classical handshake is linked. A caller must install the PQC upgrade.
func upgradeSecretConn(net.Conn, time.Duration, crypto.PrivKey) (AuthenticatedConn, error) {
	return nil, errors.New("no classical handshake is built; authenticated PQC upgrade required")
}

func (mt *MultiplexTransport) requireBuildProfileTransport() error {
	if mt.authenticatedUpgrade == nil {
		return errors.New("authenticated PQC upgrade required")
	}
	return nil
}
