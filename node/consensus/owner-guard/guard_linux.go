//go:build linux

package ownerguard

import (
	"bytes"
	"crypto/sha512"
	"errors"
	"fmt"
	"io"
	"os"
	"runtime"
	"strings"
	"sync/atomic"
	"unsafe"

	"golang.org/x/sys/unix"
)

var started atomic.Bool

// Run must be called once, directly by main. It keeps that goroutine on the
// guard-bearing OS thread until main exits. It never unlocks that thread.
// work must not return until its process-lifetime work and cleanup are complete.
func Run(role Role, work func() error) error {
	if work == nil || !started.CompareAndSwap(false, true) {
		return errAdmission
	}
	runtime.LockOSThread()
	defer unix.Close(parentFD)
	if err := admit(role); err != nil {
		return fmt.Errorf("owner startup guard: %w", err)
	}
	return work()
}
func monotonic() (int64, error) {
	var ts unix.Timespec
	if err := unix.ClockGettime(unix.CLOCK_MONOTONIC, &ts); err != nil {
		return 0, err
	}
	if ts.Sec < 0 || ts.Sec > (int64(^uint64(0)>>1)-ts.Nsec)/1000000000 {
		return 0, errAdmission
	}
	return ts.Sec*1000000000 + ts.Nsec, nil
}
func before(deadline int64) error {
	now, e := monotonic()
	if e != nil {
		return e
	}
	if now >= deadline {
		return errors.New("owner admission deadline expired")
	}
	return nil
}
func boundedFile(path string) ([]byte, error) {
	f, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	b, e := io.ReadAll(io.LimitReader(f, 16385))
	if e != nil {
		return nil, e
	}
	if len(b) > 16384 {
		return nil, errAdmission
	}
	return b, nil
}

// A closed peer can leave a complete queued packet readable. Only the read
// path permits HUP; receive still checks exact length and admit checks GO,
// the deadline, the live owner and the parent-death signal.
func pollReady(revents, wanted int16) bool {
	allowed := wanted
	if wanted == unix.POLLIN {
		allowed |= unix.POLLHUP
	}
	return revents & ^allowed == 0 && revents&wanted != 0
}
func waitIO(fd int, wanted int16, deadline int64) error {
	for {
		now, e := monotonic()
		if e != nil {
			return e
		}
		if now >= deadline {
			return errAdmission
		}
		// Round up nanoseconds. Cap each wait so deadline checks remain bounded.
		ms := (deadline-now)/1000000 + 1
		if ms > 1000 {
			ms = 1000
		}
		p := []unix.PollFd{{Fd: int32(fd), Events: wanted}}
		n, e := unix.Poll(p, int(ms))
		if e == unix.EINTR {
			continue
		}
		if e != nil {
			return e
		}
		if n == 0 {
			continue
		}
		if n != 1 || !pollReady(p[0].Revents, wanted) {
			return errAdmission
		}
		return before(deadline)
	}
}
func receive(size int, deadline int64, queued bool) ([]byte, error) {
	for {
		if !queued {
			if e := waitIO(controlFD, unix.POLLIN, deadline); e != nil {
				return nil, e
			}
		}
		b := make([]byte, size+1)
		// recvfrom has no ancillary buffer and cannot install SCM_RIGHTS descriptors.
		n, _, e := unix.Recvfrom(controlFD, b, unix.MSG_DONTWAIT|unix.MSG_TRUNC)
		if e == unix.EINTR {
			if queued {
				return nil, e
			}
			continue
		}
		if e == unix.EAGAIN && !queued {
			continue
		}
		if e != nil {
			return nil, e
		}
		if n != size {
			return nil, errAdmission
		}
		return b[:n], nil
	}
}
func send(raw []byte, deadline int64) error {
	for {
		if e := waitIO(controlFD, unix.POLLOUT, deadline); e != nil {
			return e
		}
		// The validated UNIX SOCK_SEQPACKET socket sends each packet atomically.
		e := unix.Sendto(controlFD, raw, unix.MSG_DONTWAIT|unix.MSG_NOSIGNAL, nil)
		if e == unix.EINTR || e == unix.EAGAIN {
			continue
		}
		if e != nil {
			return e
		}
		return before(deadline)
	}
}
func ownerLive(f frame) error {
	if e := before(f.deadline); e != nil {
		return e
	}
	if os.Getppid() != int(f.pid) || os.Getuid() != int(f.uid) || os.Geteuid() != int(f.uid) {
		return errAdmission
	}
	var st unix.Stat_t
	if e := unix.Fstat(parentFD, &st); e != nil {
		return e
	}
	if uint64(st.Dev) != f.device || st.Ino != f.inode {
		return errAdmission
	}
	link, e := os.Readlink("/proc/self/fd/8")
	if e != nil {
		return e
	}
	if link != "anon_inode:[pidfd]" {
		return errAdmission
	}
	info, e := boundedFile("/proc/self/fdinfo/8")
	if e != nil {
		return e
	}
	pid, e := singleDecimal(string(info), "Pid:")
	if e != nil || pid != uint64(f.pid) {
		return errAdmission
	}
	p := []unix.PollFd{{Fd: parentFD, Events: unix.POLLIN}}
	n, e := unix.Poll(p, 0)
	if e != nil {
		return e
	}
	if n != 0 || p[0].Revents != 0 {
		return errAdmission
	}
	if os.Getppid() != int(f.pid) {
		return errAdmission
	}
	return before(f.deadline)
}
func observedDeathSignal() (int, error) {
	// PR_GET_PDEATHSIG writes an int pointer; it does not return the signal.
	var value int32
	_, _, e := unix.Syscall6(unix.SYS_PRCTL, uintptr(unix.PR_GET_PDEATHSIG), uintptr(unsafe.Pointer(&value)), 0, 0, 0, 0)
	if e != 0 {
		return 0, e
	}
	return int(value), nil
}
func helperContext(expected []byte, deadline int64) error {
	if len(expected) != sha512.Size {
		return errAdmission
	}
	f, e := os.Open("/proc/self/exe")
	if e != nil {
		return e
	}
	defer f.Close()
	info, e := f.Stat()
	if e != nil {
		return e
	}
	if !info.Mode().IsRegular() || info.Size() <= 0 {
		return errAdmission
	}
	h := sha512.New()
	buffer := make([]byte, 32768)
	var count int64
	for {
		if e = before(deadline); e != nil {
			return e
		}
		n, readErr := f.Read(buffer)
		if n > 0 {
			count += int64(n)
			if count > info.Size() {
				return errAdmission
			}
			_, _ = h.Write(buffer[:n])
		}
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			return readErr
		}
		if n == 0 {
			return io.ErrNoProgress
		}
	}
	if count != info.Size() || !bytes.Equal(h.Sum(nil), expected) {
		return errAdmission
	}
	return before(deadline)
}

// A helper can execute from the supervisor under an incomplete AppArmor
// stack. Reject that process before it receives a request or sends READY.
func validateHelperLabel(raw string) error {
	if len(raw) == 0 || len(raw) > 256 || !strings.HasSuffix(raw, " (enforce)\n") {
		return errAdmission
	}
	label := strings.TrimSuffix(raw, " (enforce)\n")
	parts := strings.Split(label, "//&")
	if len(parts) != 3 {
		return errAdmission
	}
	body := strings.TrimPrefix(parts[0], "dyt-role-")
	if body == parts[0] || !strings.HasSuffix(body, "-application-owner") {
		return errAdmission
	}
	body = strings.TrimSuffix(body, "-application-owner")
	digest, unit, ok := strings.Cut(body, "-")
	if !ok || len(digest) != 20 || len(unit) == 0 || len(unit) > 32 {
		return errAdmission
	}
	for _, c := range digest {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return errAdmission
		}
	}
	for _, c := range unit {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') ||
			(c >= 'A' && c <= 'Z') || c == '_' || c == '.' || c == '-') {
			return errAdmission
		}
	}
	if parts[1] != "dyt-role-"+body+"-helper" ||
		parts[2] != "dyt-role-"+body+"-supervisor" {
		return errAdmission
	}
	return nil
}

func requireHelperLabel() error {
	raw, err := boundedFile("/proc/self/attr/current")
	if err != nil {
		return err
	}
	return validateHelperLabel(string(raw))
}
func anonymousUnix(fd int, operation uintptr) error {
	var address unix.RawSockaddrUnix
	length := uint32(unsafe.Sizeof(address))
	_, _, e := unix.Syscall(operation, uintptr(fd), uintptr(unsafe.Pointer(&address)), uintptr(unsafe.Pointer(&length)))
	if e != 0 {
		return e
	}
	// x/sys renders both unnamed and zero-length abstract names as "@".
	// Raw address length2 admits only the unnamed socketpair endpoint.
	if length != 2 || address.Family != unix.AF_UNIX {
		return errAdmission
	}
	return nil
}
func admit(role Role) (admissionErr error) {
	stage := "control_socket"
	defer func() {
		if admissionErr != nil {
			admissionErr = fmt.Errorf("%s: %w", stage, admissionErr)
		}
	}()
	defer unix.Close(controlFD)
	kind, e := unix.GetsockoptInt(controlFD, unix.SOL_SOCKET, unix.SO_TYPE)
	if e != nil {
		return e
	}
	if kind != unix.SOCK_SEQPACKET {
		return errAdmission
	}
	for _, operation := range []uintptr{unix.SYS_GETSOCKNAME, unix.SYS_GETPEERNAME} {
		if e := anonymousUnix(controlFD, operation); e != nil {
			return e
		}
	}
	stage = "bootstrap_frame"
	raw, e := receive(frameSize, 0, true)
	if e != nil {
		return e
	}
	now, e := monotonic()
	if e != nil {
		return e
	}
	f, e := parseFrame(raw, role, now)
	if e != nil {
		return e
	}
	stage = "control_peer"
	peer, e := unix.GetsockoptUcred(controlFD, unix.SOL_SOCKET, unix.SO_PEERCRED)
	if e != nil {
		return e
	}
	if peer.Pid != int32(f.pid) || peer.Uid != f.uid {
		return errAdmission
	}
	stage = "self_status"
	status, e := boundedFile("/proc/self/status")
	if e != nil {
		return e
	}
	if e = validateStatus(string(status)); e != nil {
		return e
	}
	if _, e = unix.FcntlInt(uintptr(parentFD), unix.F_SETFD, unix.FD_CLOEXEC); e != nil {
		return e
	}
	stage = "owner_identity"
	if e = ownerLive(f); e != nil {
		return e
	}
	if f.role == Helper {
		stage = "helper_apparmor_label"
		if e = requireHelperLabel(); e != nil {
			return e
		}
		stage = "helper_executable_context"
		if e = helperContext(f.raw[48:112], f.deadline); e != nil {
			return e
		}
	}
	stage = "signal_establishment"
	value, e := observedDeathSignal()
	if e != nil {
		return e
	}
	if value != 0 && value != int(unix.SIGKILL) {
		return errAdmission
	}
	if e = unix.Prctl(unix.PR_SET_PDEATHSIG, uintptr(unix.SIGKILL), 0, 0, 0); e != nil {
		return e
	}
	value, e = observedDeathSignal()
	if e != nil {
		return e
	}
	if value != int(unix.SIGKILL) {
		return errAdmission
	}
	if e = ownerLive(f); e != nil {
		return e
	}
	stage = "ready_and_release"
	copy(f.raw[:8], []byte("DYTRDY01"))
	if e = send(f.raw[:], f.deadline); e != nil {
		return e
	}
	goToken, e := receive(8, f.deadline, false)
	if e != nil {
		return e
	}
	if !bytes.Equal(goToken, []byte("DYTGO001")) {
		return errAdmission
	}
	if e = ownerLive(f); e != nil {
		return e
	}
	value, e = observedDeathSignal()
	if e != nil {
		return e
	}
	if value != int(unix.SIGKILL) {
		return errAdmission
	}
	return before(f.deadline)
}
