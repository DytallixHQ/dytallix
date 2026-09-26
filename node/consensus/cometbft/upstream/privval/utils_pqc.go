//go:build dytallix_pqc_only

package privval

import (
	"errors"
	"fmt"
	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/libs/log"
	cmtnet "github.com/cometbft/cometbft/libs/net"
)

// IsConnTimeout returns a boolean indicating whether the error is known to
// report that a connection timeout occurred. This detects both fundamental
// network timeouts, as well as ErrConnTimeout errors.
func IsConnTimeout(err error) bool {
	_, ok := errors.Unwrap(err).(timeoutError)
	switch {
	case errors.As(err, &EndpointTimeoutError{}):
		return true
	case ok:
		return true
	default:
		return false
	}
}

func NewSignerListener(string, log.Logger) (*SignerListenerEndpoint, error) {
	return nil, errors.New("remote signing excluded by dytallix_pqc_only")
}
func NewSignerListenerFromAddr(string, crypto.PrivKey, log.Logger) (*SignerListenerEndpoint, error) {
	return nil, errors.New("remote signing excluded by dytallix_pqc_only")
}

// GetFreeLocalhostAddrPort returns a free localhost:port address
func GetFreeLocalhostAddrPort() string {
	port, err := cmtnet.GetFreePort()
	if err != nil {
		panic(err)
	}
	return fmt.Sprintf("127.0.0.1:%d", port)
}
