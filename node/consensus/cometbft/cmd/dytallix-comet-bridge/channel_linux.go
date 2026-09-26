//go:build linux

package main

import (
	"bufio"
	"errors"
	"fmt"
	"os"

	"dytallix.local/consensus/cometbft/internal/startupdiag"
	"golang.org/x/sys/unix"
)

// Descriptor validation establishes channel shape, not executable identity.
// The native owner must retain the actual application Child and observe it.
func validatePipe(fd, direction int) (unix.Stat_t, error) {
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return stat, err
	}
	if stat.Mode&unix.S_IFMT != unix.S_IFIFO || stat.Mode&0o777 != 0o600 || stat.Uid != uint32(os.Geteuid()) {
		return stat, startupdiag.AsClass(startupdiag.Protocol, errors.New("application descriptor must be an owned private pipe"))
	}
	flags, err := unix.FcntlInt(uintptr(fd), unix.F_GETFL, 0)
	if err != nil {
		return stat, err
	}
	if flags&unix.O_ACCMODE != direction {
		return stat, startupdiag.AsClass(startupdiag.Protocol, errors.New("application pipe direction mismatch"))
	}
	target, err := os.Readlink(fmt.Sprintf("/proc/self/fd/%d", fd))
	if err != nil {
		return stat, err
	}
	if target != fmt.Sprintf("pipe:[%d]", stat.Ino) {
		return stat, startupdiag.AsClass(startupdiag.Protocol, errors.New("application descriptor is not an anonymous pipe"))
	}
	return stat, nil
}

// Ownership transfers only after both descriptors pass validation. The caller
// must pass exclusive bridge endpoints, without other local os.File wrappers.
func inheritedApplication(input, output int) (*child, error) {
	if input < 3 || output < 3 || input == output {
		return nil, startupdiag.AsClass(startupdiag.Protocol, errors.New("application pipe descriptors must be distinct and >= 3"))
	}
	inStat, err := validatePipe(input, unix.O_WRONLY)
	if err != nil {
		return nil, err
	}
	outStat, err := validatePipe(output, unix.O_RDONLY)
	if err != nil {
		return nil, err
	}
	if inStat.Dev == outStat.Dev && inStat.Ino == outStat.Ino {
		return nil, startupdiag.AsClass(startupdiag.Protocol, errors.New("application channels require distinct pipes"))
	}
	// Make the adopted descriptors pollable so Close interrupts blocked I/O.
	// All following failures close both transferred endpoints.
	failed := true
	defer func() {
		if failed {
			_ = unix.Close(input)
			_ = unix.Close(output)
		}
	}()
	for _, fd := range []int{input, output} {
		if err := unix.SetNonblock(fd, true); err != nil {
			return nil, err
		}
		if _, err := unix.FcntlInt(uintptr(fd), unix.F_SETFD, unix.FD_CLOEXEC); err != nil {
			return nil, err
		}
	}
	in := os.NewFile(uintptr(input), "inherited-application-input")
	out := os.NewFile(uintptr(output), "inherited-application-output")
	scan := bufio.NewScanner(out)
	scan.Buffer(make([]byte, 64<<10), maxJSONBytes)
	c := &child{input: in, output: out, scan: scan, done: make(chan error, 1)}
	failed = false
	return c, nil
}
