//go:build !dytallix_pqc_only

package p2p

import (
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/p2p/conn"
	"net"
	"time"
)

func upgradeSecretConn(
	c net.Conn,
	timeout time.Duration,
	privKey crypto.PrivKey,
) (*conn.SecretConnection, error) {
	if err := c.SetDeadline(time.Now().Add(timeout)); err != nil {
		return nil, err
	}

	sc, err := conn.MakeSecretConnection(c, privKey)
	if err != nil {
		return nil, err
	}

	return sc, sc.SetDeadline(time.Time{})
}

func (mt *MultiplexTransport) requireBuildProfileTransport() error { return nil }
