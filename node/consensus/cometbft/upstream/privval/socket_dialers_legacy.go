//go:build !dytallix_pqc_only

package privval

import (
	"github.com/cometbft/cometbft/crypto"
	cmtnet "github.com/cometbft/cometbft/libs/net"
	p2pconn "github.com/cometbft/cometbft/p2p/conn"
	"net"
	"time"
)

// DialTCPFn dials the given tcp addr, using the given timeoutReadWrite and
// privKey for the authenticated encryption handshake.
func DialTCPFn(addr string, timeoutReadWrite time.Duration, privKey crypto.PrivKey) SocketDialer {
	return func() (net.Conn, error) {
		conn, err := cmtnet.Connect(addr)
		if err == nil {
			deadline := time.Now().Add(timeoutReadWrite)
			err = conn.SetDeadline(deadline)
		}
		if err == nil {
			conn, err = p2pconn.MakeSecretConnection(conn, privKey)
		}
		return conn, err
	}
}
