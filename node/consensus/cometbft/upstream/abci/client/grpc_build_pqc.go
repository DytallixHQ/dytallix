package abcicli

import "errors"

func newGRPCClientForBuild(string, bool) (Client, error) {
	return nil, errors.New("gRPC ABCI is not built (Dytallix PQC-only fork); socket transport required")
}
