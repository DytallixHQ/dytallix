//go:build dytallix_pqc_only

package abcicli

import "errors"

func newGRPCClientForBuild(string, bool) (Client, error) {
	return nil, errors.New("gRPC ABCI excluded by dytallix_pqc_only; socket transport required")
}
