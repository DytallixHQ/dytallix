package server

import (
	"errors"
	"github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/libs/service"
)

func newGRPCServerForBuild(string, types.Application) (service.Service, error) {
	return nil, errors.New("gRPC ABCI server is not built (Dytallix PQC-only fork)")
}
