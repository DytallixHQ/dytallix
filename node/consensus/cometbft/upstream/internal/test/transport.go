package test

import "testing"

// SkipWithoutPQCUpgrade skips a test that connects switches through the p2p
// test helpers (MakeSwitch, MakeConnectedSwitches, Connect2Switches). The
// PQC-only transport refuses a peer without the authenticated PQC upgrade,
// which only the engine installs, and the helpers have no test upgrade
// (E04 gap 20). Remove the call when they gain one.
func SkipWithoutPQCUpgrade(t *testing.T) {
	t.Helper()
	t.Skip("needs an authenticated PQC upgrade in the p2p test helpers")
}
