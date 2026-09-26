//go:build dytallix_pqc_only

package server

import (
	"errors"
	"github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/libs/service"
)

func newGRPCServerForBuild(string, types.Application) (service.Service, error) {
	return nil, errors.New("gRPC ABCI server excluded by dytallix_pqc_only")
}
