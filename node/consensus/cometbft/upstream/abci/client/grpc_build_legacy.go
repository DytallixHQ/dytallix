//go:build !dytallix_pqc_only

package abcicli

func newGRPCClientForBuild(addr string, mustConnect bool) (Client, error) {
	return NewGRPCClient(addr, mustConnect), nil
}
