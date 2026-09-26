//go:build dytallix_pqc_only

package privval

import (
	"errors"
	"github.com/cometbft/cometbft/crypto"
	"net"
	"time"
)

func DialTCPFn(string, time.Duration, crypto.PrivKey) SocketDialer {
	return func() (net.Conn, error) { return nil, errors.New("remote signing excluded by dytallix_pqc_only") }
}
