//go:build dytallix_pqc_ipc && !dytallix_pqc_only

package main

// IPC builds require the existing PQC exclusion profile as well.
var _ = dytallix_pqc_ipc_requires_dytallix_pqc_only
