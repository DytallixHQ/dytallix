//go:build !production

package main

import "testing"

func TestTransportProfileRefusesProductionAndUnimplementedPQC(t *testing.T) {
	for _, tc := range []struct {
		profile    string
		production bool
		allow      bool
	}{
		{"legacy-cometbft-loopback-only", false, false},
		{pqcLoopbackTransport, false, true},
		{pqcSeedLoopbackTransport, false, true},
		{pqcSeedLoopbackTransport, true, false},
		{pqcPrivateSeedTransport, false, true},
		{pqcPrivateSeedTransport, true, false},
		{"mlkem768-mldsa65", false, false},
		{"", false, false},
		{"production", true, false},
	} {
		err := checkTransportProfile(tc.profile, tc.production)
		if (err == nil) != tc.allow {
			t.Fatalf("profile %q production %v: %v", tc.profile, tc.production, err)
		}
	}
}
