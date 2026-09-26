package main

import (
	"errors"
	"os"
	"testing"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
)

func TestProductionFlagRemainsBlockedWithCandidateStaging(t *testing.T) {
	prior := os.Args
	t.Cleanup(func() { os.Args = prior })
	os.Args = []string{"dytallix-pqc-engine", "start", "--home", t.TempDir(), "--p2p-profile", "dytallix-pqc-production-candidate-v1", "--candidate-staging", "--production"}
	if err := run(); !errors.Is(err, pqcp2p.ErrProductionBlocked) {
		t.Fatalf("production flag did not refuse candidate startup: %v", err)
	}
}

func TestCandidateProfileRequiresExplicitStagingFlag(t *testing.T) {
	prior := os.Args
	t.Cleanup(func() { os.Args = prior })
	os.Args = []string{"dytallix-pqc-engine", "start", "--home", t.TempDir(), "--p2p-profile", "dytallix-pqc-production-candidate-v1"}
	if err := run(); err == nil || errors.Is(err, pqcp2p.ErrProductionBlocked) {
		t.Fatalf("candidate profile bypassed the staging route: %v", err)
	}
}
