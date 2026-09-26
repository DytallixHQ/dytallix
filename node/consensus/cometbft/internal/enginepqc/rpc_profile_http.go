//go:build !dytallix_pqc_ipc

package enginepqc

import "errors"

func ConfigureRPCProfile(_ *Runtime, profile string) error {
	if profile != "" {
		return errors.New("RPC profile requires the explicit IPC build")
	}
	return nil
}

const RPCBuildProfile = "existing-http-rpc"
