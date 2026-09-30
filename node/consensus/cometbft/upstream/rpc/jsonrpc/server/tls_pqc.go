package server

import (
	"errors"
	"github.com/cometbft/cometbft/libs/log"
	"net"
	"net/http"
)

func ServeTLS(net.Listener, http.Handler, string, string, log.Logger, *Config) error {
	return errors.New("RPC TLS is not built (Dytallix PQC-only fork)")
}
