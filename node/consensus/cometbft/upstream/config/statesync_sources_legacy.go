//go:build !dytallix_pqc_only

package config

import cmterrors "github.com/cometbft/cometbft/types/errors"

// validateStateSyncSources requires the light client's RPC servers.
func validateStateSyncSources(servers []string) error {
	if len(servers) == 0 {
		return cmterrors.ErrRequiredField{Field: "rpc_servers"}
	}
	if len(servers) < 2 {
		return ErrNotEnoughRPCServers
	}
	for _, server := range servers {
		if len(server) == 0 {
			return ErrEmptyRPCServerEntry
		}
	}
	return nil
}
