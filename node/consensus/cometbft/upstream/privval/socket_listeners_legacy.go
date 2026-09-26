//go:build !dytallix_pqc_only

package privval

import (
	"github.com/cometbft/cometbft/crypto/ed25519"
	p2pconn "github.com/cometbft/cometbft/p2p/conn"
	"net"
	"time"
)

// TCPListenerOption sets an optional parameter on the tcpListener.
type TCPListenerOption func(*TCPListener)

// TCPListenerTimeoutAccept sets the timeout for the listener.
// A zero time value disables the timeout.
func TCPListenerTimeoutAccept(timeout time.Duration) TCPListenerOption {
	return func(tl *TCPListener) { tl.timeoutAccept = timeout }
}

// TCPListenerTimeoutReadWrite sets the read and write timeout for connections
// from external signing processes.
func TCPListenerTimeoutReadWrite(timeout time.Duration) TCPListenerOption {
	return func(tl *TCPListener) { tl.timeoutReadWrite = timeout }
}

// tcpListener implements net.Listener.
var _ net.Listener = (*TCPListener)(nil)

// TCPListener wraps a *net.TCPListener to standardize protocol timeouts
// and potentially other tuning parameters. It also returns encrypted connections.
type TCPListener struct {
	*net.TCPListener

	secretConnKey ed25519.PrivKey

	timeoutAccept    time.Duration
	timeoutReadWrite time.Duration
}

// NewTCPListener returns a listener that accepts authenticated encrypted connections
// using the given secretConnKey and the default timeout values.
func NewTCPListener(ln net.Listener, secretConnKey ed25519.PrivKey) *TCPListener {
	return &TCPListener{
		TCPListener:      ln.(*net.TCPListener),
		secretConnKey:    secretConnKey,
		timeoutAccept:    time.Second * defaultTimeoutAcceptSeconds,
		timeoutReadWrite: time.Second * defaultTimeoutReadWriteSeconds,
	}
}

// Accept implements net.Listener.
func (ln *TCPListener) Accept() (net.Conn, error) {
	deadline := time.Now().Add(ln.timeoutAccept)
	err := ln.SetDeadline(deadline)
	if err != nil {
		return nil, err
	}

	tc, err := ln.AcceptTCP()
	if err != nil {
		return nil, err
	}

	// Wrap the conn in our timeout and encryption wrappers
	timeoutConn := newTimeoutConn(tc, ln.timeoutReadWrite)
	secretConn, err := p2pconn.MakeSecretConnection(timeoutConn, ln.secretConnKey)
	if err != nil {
		_ = timeoutConn.Close()
		return nil, err
	}

	return secretConn, nil
}

//------------------------------------------------------------------
