//go:build linux

package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

// Raw bridge descriptors have no competing os.File finalizer. The owner-side
// endpoints remain independent and are never adopted by the bridge.
func pipeFixture(t *testing.T) (int, int, *os.File, *os.File) {
	t.Helper()
	in, out := make([]int, 2), make([]int, 2)
	if err := unix.Pipe2(in, unix.O_CLOEXEC); err != nil {
		t.Fatal(err)
	}
	if err := unix.Pipe2(out, unix.O_CLOEXEC); err != nil {
		t.Fatal(err)
	}
	reader, writer := os.NewFile(uintptr(in[0]), "owner-request"), os.NewFile(uintptr(out[1]), "owner-response")
	t.Cleanup(func() { _ = reader.Close(); _ = writer.Close() })
	return in[1], out[0], reader, writer
}

func inheritedFixture(t *testing.T) (*child, *os.File, *os.File, int, int) {
	t.Helper()
	in, out, reader, writer := pipeFixture(t)
	c, err := inheritedApplication(in, out)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(c.closeInherited)
	return c, reader, writer, in, out
}

func TestInheritedRealPipeRoundTripAndOwnedCleanup(t *testing.T) {
	c, reader, writer, in, out := inheritedFixture(t)
	served := make(chan error, 1)
	go func() {
		line, err := bufio.NewReader(reader).ReadBytes('\n')
		if err != nil {
			served <- err
			return
		}
		var request struct {
			Method  string         `json:"method"`
			Payload map[string]int `json:"payload"`
		}
		if err = json.Unmarshal(line, &request); err != nil {
			served <- err
			return
		}
		if request.Method != "fixture" || request.Payload["height"] != 7 {
			served <- errors.New("request changed")
			return
		}
		_, err = io.WriteString(writer, "{\"ok\":true,\"result\":{\"height\":7}}\n")
		served <- err
	}()
	var result map[string]int
	if err := c.call(context.Background(), "fixture", map[string]int{"height": 7}, &result); err != nil {
		t.Fatal(err)
	}
	if err := <-served; err != nil {
		t.Fatal(err)
	}
	if result["height"] != 7 || c.cmd != nil {
		t.Fatal("incorrect response or process ownership")
	}
	for _, fd := range []int{in, out} {
		flags, err := unix.FcntlInt(uintptr(fd), unix.F_GETFD, 0)
		if err != nil || flags&unix.FD_CLOEXEC == 0 {
			t.Fatal("descriptor can leak through exec")
		}
	}
	c.closeInherited()
	c.closeInherited()
	for _, fd := range []int{in, out} {
		var st unix.Stat_t
		if !errors.Is(unix.Fstat(fd, &st), unix.EBADF) {
			t.Fatal("adopted endpoint remained open")
		}
	}
	if _, err := reader.Stat(); err != nil {
		t.Fatal("owner reader closed", err)
	}
	if _, err := writer.Stat(); err != nil {
		t.Fatal("owner writer closed", err)
	}
	var one [1]byte
	if _, err := reader.Read(one[:]); err != io.EOF {
		t.Fatal("owner did not observe closed bridge input", err)
	}
}

func TestInheritedRejectsWrongDescriptorShape(t *testing.T) {
	for _, kind := range []string{"same-number", "stdio", "closed", "reverse", "same-pipe", "file", "named-fifo", "public-pipe"} {
		t.Run(kind, func(t *testing.T) {
			in, out, reader, _ := pipeFixture(t)
			defer unix.Close(in)
			defer unix.Close(out)
			left, right := in, out
			switch kind {
			case "same-number":
				right = in
			case "stdio":
				left = 1
			case "closed":
				left = 999999
			case "reverse":
				left, right = out, in
			case "same-pipe":
				right = int(reader.Fd())
			case "file":
				f, err := os.CreateTemp(t.TempDir(), "file")
				if err != nil {
					t.Fatal(err)
				}
				defer f.Close()
				left = int(f.Fd())
			case "named-fifo":
				path := filepath.Join(t.TempDir(), "fifo")
				if err := unix.Mkfifo(path, 0o600); err != nil {
					t.Fatal(err)
				}
				fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NONBLOCK, 0)
				if err != nil {
					t.Fatal(err)
				}
				defer unix.Close(fd)
				right = fd
			case "public-pipe":
				if err := unix.Fchmod(in, 0o666); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := inheritedApplication(left, right); err == nil {
				t.Fatal("accepted malformed descriptors")
			}
			// Validation failure must not adopt or close either supplied endpoint.
			var st unix.Stat_t
			if unix.Fstat(in, &st) != nil || unix.Fstat(out, &st) != nil {
				t.Fatal("validation failure closed caller descriptors")
			}
		})
	}
}

func TestInheritedEOFClosesChannelsAndSignalsExit(t *testing.T) {
	c, _, writer, _, _ := inheritedFixture(t)
	_ = writer.Close()
	var result struct{}
	err := c.call(context.Background(), "fixture", struct{}{}, &result)
	if !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatal(err)
	}
	select {
	case <-c.done:
	case <-time.After(time.Second):
		t.Fatal("channel failure did not signal bridge exit")
	}
	if c.call(context.Background(), "fixture", struct{}{}, &result) != err {
		t.Fatal("failed stream reused")
	}
}

func TestInheritedCancellationInterruptsBlockedResponse(t *testing.T) {
	c, reader, _, _, _ := inheritedFixture(t)
	read := make(chan error, 1)
	go func() { _, err := bufio.NewReader(reader).ReadBytes('\n'); read <- err }()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { var result struct{}; done <- c.call(ctx, "fixture", struct{}{}, &result) }()
	if err := <-read; err != nil {
		t.Fatal(err)
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("cancel did not interrupt call")
	}
	select {
	case <-c.done:
	case <-time.After(time.Second):
		t.Fatal("cancel did not signal exit")
	}
}

func TestInheritedCancellationInterruptsBlockedRequest(t *testing.T) {
	c, _, _, in, _ := inheritedFixture(t)
	if _, err := unix.FcntlInt(uintptr(in), unix.F_SETPIPE_SZ, 4096); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	var result struct{}
	err := c.call(ctx, "fixture", strings.Repeat("x", 1<<20), &result)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	select {
	case <-c.done:
	case <-time.After(time.Second):
		t.Fatal("blocked-write cancellation did not signal exit")
	}
}

func TestInheritedOwnedCleanupUnblocksPendingExchange(t *testing.T) {
	c, reader, _, _, _ := inheritedFixture(t)
	read := make(chan error, 1)
	go func() { _, err := bufio.NewReader(reader).ReadBytes('\n'); read <- err }()
	done := make(chan error, 1)
	go func() {
		var result struct{}
		done <- c.call(context.Background(), "fixture", struct{}{}, &result)
	}()
	if err := <-read; err != nil {
		t.Fatal(err)
	}
	c.closeInherited()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("closed channel accepted response")
		}
	case <-time.After(time.Second):
		t.Fatal("owned cleanup did not unblock exchange")
	}
}
