//go:build linux

package ownerguard

import (
	"bytes"
	"context"
	"crypto/sha512"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

func TestLinuxUnnamedSocketCheck(t *testing.T) {
	pair, e := unix.Socketpair(unix.AF_UNIX, unix.SOCK_SEQPACKET|unix.SOCK_CLOEXEC, 0)
	if e != nil {
		t.Fatal(e)
	}
	defer unix.Close(pair[0])
	defer unix.Close(pair[1])
	if e = anonymousUnix(pair[0], unix.SYS_GETSOCKNAME); e != nil {
		t.Fatal(e)
	}
	if e = anonymousUnix(pair[0], unix.SYS_GETPEERNAME); e != nil {
		t.Fatal(e)
	}
	named, e := unix.Socket(unix.AF_UNIX, unix.SOCK_SEQPACKET|unix.SOCK_CLOEXEC, 0)
	if e != nil {
		t.Fatal(e)
	}
	defer unix.Close(named)
	if e = unix.Bind(named, &unix.SockaddrUnix{Name: fmt.Sprintf("@dyt-own-test-%d", os.Getpid())}); e != nil {
		t.Fatal(e)
	}
	if e = anonymousUnix(named, unix.SYS_GETSOCKNAME); e == nil {
		t.Fatal("named endpoint admitted")
	}
	if e = anonymousUnix(-1, unix.SYS_GETSOCKNAME); e == nil {
		t.Fatal("invalid descriptor admitted")
	}
}
func TestLinuxMissingBootstrapRefusesWork(t *testing.T) {
	if os.Getenv("DYT_OWNER_GUARD_TEST_CHILD") == "1" {
		e := Run(Engine, func() error { fmt.Fprint(os.Stdout, "UNGUARDED_WORK"); return nil })
		if e != nil {
			fmt.Fprint(os.Stderr, e)
			os.Exit(23)
		}
		os.Exit(24)
	}
	binary, e := os.Executable()
	if e != nil {
		t.Fatal(e)
	}
	cmd := exec.Command(binary, "-test.run=^TestLinuxMissingBootstrapRefusesWork$")
	cmd.Env = append(os.Environ(), "DYT_OWNER_GUARD_TEST_CHILD=1")
	out, e := cmd.CombinedOutput()
	var exit *exec.ExitError
	if e == nil {
		t.Fatal("missing bootstrap admitted")
	}
	exit, ok := e.(*exec.ExitError)
	if !ok || exit.ExitCode() != 23 || bytes.Contains(out, []byte("UNGUARDED_WORK")) || !bytes.Contains(out, []byte("owner startup guard")) {
		t.Fatalf("unexpected refusal %v: %s", e, out)
	}
}

func TestLinuxHelperContextBindsActualExecutable(t *testing.T) {
	raw, e := os.ReadFile("/proc/self/exe")
	if e != nil {
		t.Fatal(e)
	}
	digest := sha512.Sum512(raw)
	now, e := monotonic()
	if e != nil {
		t.Fatal(e)
	}
	deadline := now + int64(5*time.Second)
	if e = helperContext(digest[:], deadline); e != nil {
		t.Fatal(e)
	}
	digest[0] ^= 1
	if e = helperContext(digest[:], deadline); e == nil {
		t.Fatal("wrong executable digest admitted")
	}
	if e = helperContext(digest[:63], deadline); e == nil {
		t.Fatal("short context admitted")
	}
	if e = helperContext(digest[:], now); e == nil {
		t.Fatal("expired deadline admitted")
	}
}

func TestLinuxHelperLabelRequiresOwnerComponent(t *testing.T) {
	base := "dyt-role-0123456789abcdefabcd-node0-"
	valid := base + "application-owner//&" + base + "helper//&" + base + "supervisor (enforce)\n"
	if e := validateHelperLabel(valid); e != nil {
		t.Fatal(e)
	}
	for _, invalid := range []string{
		base + "helper//&" + base + "supervisor (enforce)\n",
		base + "application-owner//&" + base + "supervisor (enforce)\n",
		base + "application-owner//&" + base + "helper//&" +
			"dyt-role-0123456789abcdefabcd-node1-supervisor (enforce)\n",
		strings.TrimSuffix(valid, " (enforce)\n") + " (complain)\n",
		valid + "extra",
	} {
		if e := validateHelperLabel(invalid); e == nil {
			t.Fatalf("incomplete or invalid stack admitted: %q", invalid)
		}
	}
}

func TestLinuxPollReadyRejectsUnrequestedEvents(t *testing.T) {
	for _, tc := range []struct {
		mask, wanted int16
		accepted     bool
	}{
		{unix.POLLIN, unix.POLLIN, true},
		{unix.POLLIN | unix.POLLHUP, unix.POLLIN, true},
		{unix.POLLHUP, unix.POLLIN, false},
		{0, unix.POLLIN, false},
		{unix.POLLIN | unix.POLLERR, unix.POLLIN, false},
		{unix.POLLIN | unix.POLLNVAL, unix.POLLIN, false},
		{unix.POLLIN | unix.POLLOUT, unix.POLLIN, false},
		{unix.POLLOUT, unix.POLLOUT, true},
		{unix.POLLOUT | unix.POLLHUP, unix.POLLOUT, false},
		{unix.POLLOUT | unix.POLLERR, unix.POLLOUT, false},
		{unix.POLLOUT | unix.POLLNVAL, unix.POLLOUT, false},
	} {
		if got := pollReady(tc.mask, tc.wanted); got != tc.accepted {
			t.Fatalf("mask=%d wanted=%d got=%v", tc.mask, tc.wanted, got)
		}
	}
}

func TestLinuxQueuedReleaseAfterClose(t *testing.T) {
	if mode := os.Getenv("DYT_QUEUED_RELEASE_TEST"); mode != "" {
		now, e := monotonic()
		if e != nil {
			t.Fatal(e)
		}
		deadline := now + int64(time.Second)
		if mode == "expired" {
			deadline = now
		}
		raw, e := receive(8, deadline, false)
		accepted := e == nil && bytes.Equal(raw, []byte("DYTGO001"))
		if accepted != (mode == "valid") {
			t.Fatalf("mode=%s accepted=%v error=%v", mode, accepted, e)
		}
		return
	}
	for _, tc := range []struct {
		name    string
		payload []byte
	}{
		{"valid", []byte("DYTGO001")}, {"empty", nil}, {"wrong", []byte("WRONG000")},
		{"short", []byte("short")}, {"oversized", []byte("DYTGO001x")}, {"expired", []byte("DYTGO001")},
	} {
		t.Run(tc.name, func(t *testing.T) {
			pair, e := unix.Socketpair(unix.AF_UNIX, unix.SOCK_SEQPACKET|unix.SOCK_CLOEXEC, 0)
			if e != nil {
				t.Fatal(e)
			}
			peer := os.NewFile(uintptr(pair[0]), "test-peer")
			defer peer.Close()
			control := os.NewFile(uintptr(pair[1]), "test-control")
			defer control.Close()
			if tc.payload != nil {
				if e = unix.Sendto(pair[0], tc.payload, unix.MSG_NOSIGNAL, nil); e != nil {
					t.Fatal(e)
				}
			}
			if e = peer.Close(); e != nil {
				t.Fatal(e)
			}
			// Linux must report readable plus HUP before the child reads the packet.
			poll := []unix.PollFd{{Fd: int32(pair[1]), Events: unix.POLLIN}}
			if n, e := unix.Poll(poll, 0); e != nil || n != 1 || poll[0].Revents != (unix.POLLIN|unix.POLLHUP) {
				t.Fatalf("unexpected poll %v %v", poll, e)
			}
			exe, e := os.Executable()
			if e != nil {
				t.Fatal(e)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, exe, "-test.run=^TestLinuxQueuedReleaseAfterClose$")
			cmd.Env = append(os.Environ(), "DYT_QUEUED_RELEASE_TEST="+tc.name)
			// ExtraFiles maps four harmless placeholders to 3..6 and control to 7.
			null, e := os.Open(os.DevNull)
			if e != nil {
				t.Fatal(e)
			}
			defer null.Close()
			cmd.ExtraFiles = []*os.File{null, null, null, null, control}
			if out, e := cmd.CombinedOutput(); e != nil {
				t.Fatalf("child failed: %v %s", e, out)
			}
		})
	}
}
