//go:build dytallix_pqc_ipc && dytallix_pqc_only

package enginepqc

import (
	"errors"
	"path/filepath"
)

const RPCBuildProfile = "dytallix-pqc-unix-v1"

func ConfigureRPCProfile(runtime *Runtime, profile string) error {
	if runtime == nil || runtime.Config == nil {
		return errors.New("existing validated engine runtime is required")
	}
	if profile != RPCBuildProfile {
		return errors.New("explicit experimental RPC profile dytallix-pqc-unix-v1 is required")
	}
	runtime.Config.RPC.ListenAddress = "unix://" + filepath.Join(runtime.Config.RootDir, "data", "rpc.sock")
	return nil
}
