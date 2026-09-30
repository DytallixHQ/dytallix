package config

import "errors"

// validateStateSyncSources refuses RPC servers: the fork has no HTTP light
// client, and the engine injects a state provider over light blocks the
// operator supplies locally (Dytallix state sync v1, rule 5).
func validateStateSyncSources(servers []string) error {
	if len(servers) != 0 {
		return errors.New("Dytallix state sync takes operator light blocks, not rpc_servers")
	}
	return nil
}
