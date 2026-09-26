//go:build dytallix_pqc_ipc && dytallix_pqc_only

package enginepqc

import (
	cfg "github.com/cometbft/cometbft/config"
	"path/filepath"
	"testing"
)

func TestIPCProfileRequiresExplicitChoice(t *testing.T) {
	for _, profile := range []string{"", "production", "existing-http-rpc"} {
		runtime := &Runtime{Config: cfg.DefaultConfig().SetRoot("/private/tmp/profile")}
		before := runtime.Config.RPC.ListenAddress
		if ConfigureRPCProfile(runtime, profile) == nil || runtime.Config.RPC.ListenAddress != before {
			t.Fatal("invalid profile changed runtime")
		}
	}
	if ConfigureRPCProfile(nil, RPCBuildProfile) == nil || ConfigureRPCProfile(&Runtime{}, RPCBuildProfile) == nil {
		t.Fatal("incomplete runtime accepted")
	}
	runtime := &Runtime{Config: cfg.DefaultConfig().SetRoot("/private/tmp/profile")}
	p2p := runtime.Config.P2P.ListenAddress
	if err := ConfigureRPCProfile(runtime, RPCBuildProfile); err != nil {
		t.Fatal(err)
	}
	if runtime.Config.RPC.ListenAddress != "unix://"+filepath.Join(runtime.Config.RootDir, "data", "rpc.sock") || runtime.Config.P2P.ListenAddress != p2p {
		t.Fatal("unexpected profile mutation")
	}
}
