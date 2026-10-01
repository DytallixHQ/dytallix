//go:build production

package enginepqc

import (
	"strings"
	"testing"
)

// A production build loads no development or staging profile (production
// activation v1, A1): every current profile is refused before any file is read.
func TestProductionBuildRefusesEveryCurrentProfile(t *testing.T) {
	if !ProductionBuild {
		t.Fatal("the production tag must set ProductionBuild")
	}
	for _, profile := range []string{Profile, SeedProfile, RemoteSeedProfile} {
		if _, err := Load("/nonexistent", profile); err == nil || !strings.Contains(err.Error(), "step A4") {
			t.Fatalf("%s: %v", profile, err)
		}
	}
	if _, err := LoadCandidateForStaging("/nonexistent"); err == nil || !strings.Contains(err.Error(), "step A4") {
		t.Fatalf("candidate staging: %v", err)
	}
}
