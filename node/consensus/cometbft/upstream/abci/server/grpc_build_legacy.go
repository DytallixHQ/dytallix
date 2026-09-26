//go:build !dytallix_pqc_only

package server

import (
	"github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/libs/service"
)

func newGRPCServerForBuild(addr string, app types.Application) (service.Service, error) {
	return NewGRPCServer(addr, app), nil
}
