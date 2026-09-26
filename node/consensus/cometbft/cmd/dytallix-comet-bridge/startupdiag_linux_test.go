//go:build linux

package main

import (
	"dytallix.local/consensus/cometbft/internal/startupdiag"
	"golang.org/x/sys/unix"
	"testing"
)

func TestPipeInvariantDiagnosticClass(t *testing.T) {
	_, err := inheritedApplication(2, 2)
	if startupdiag.ExitCode(startupdiag.At(3, err)) != 62 {
		t.Fatal("descriptor invariant not protocol")
	}
	in, out, _, _ := pipeFixture(t)
	defer unix.Close(in)
	defer unix.Close(out)
	_, err = validatePipe(in, unix.O_RDONLY)
	if startupdiag.Classify(err) != startupdiag.Protocol {
		t.Fatal("direction invariant not protocol")
	}
	if err = unix.Fchmod(in, 0644); err != nil {
		t.Fatal(err)
	}
	_, err = validatePipe(in, unix.O_WRONLY)
	if startupdiag.Classify(err) != startupdiag.Protocol {
		t.Fatal("privacy invariant not protocol")
	}
	_, err = validatePipe(-1, unix.O_RDONLY)
	if startupdiag.Classify(err) != startupdiag.Other {
		t.Fatal("syscall failure falsely classified as invariant")
	}
}
