package startupdiag

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"syscall"
	"testing"
)

func TestEveryExitMapping(t *testing.T) {
	sentinel := errors.New("private text must not influence classification")
	for stage := uint8(0); stage < 8; stage++ {
		for class := Other; class <= Cancelled; class++ {
			err := At(stage, AsClass(class, sentinel))
			if got, want := ExitCode(err), 32+8*int(stage)+int(class); got != want {
				t.Fatalf("stage %d class %d got %d want %d", stage, class, got, want)
			}
			if !errors.Is(err, sentinel) {
				t.Fatal("lost original cause")
			}
		}
	}
	if ExitCode(nil) != 0 || At(1, nil) != nil || AsClass(Protocol, nil) != nil {
		t.Fatal("nil changed")
	}
	if ExitCode(sentinel) != 32 {
		t.Fatal("untyped owner error must use guard/other")
	}
}

func TestTypedClassification(t *testing.T) {
	cases := []struct {
		err   error
		class Class
	}{
		{syscall.EACCES, Permission}, {syscall.EPERM, Permission}, {syscall.ENOENT, NotFound},
		{syscall.EINVAL, Invalid}, {context.DeadlineExceeded, Timeout}, {os.ErrDeadlineExceeded, Timeout}, {syscall.ETIMEDOUT, Timeout},
		{syscall.ENOMEM, Resource}, {syscall.ENOSPC, Resource}, {syscall.EMFILE, Resource}, {syscall.ENFILE, Resource}, {syscall.EAGAIN, Resource},
		{syscall.EPROTO, Protocol}, {syscall.EPIPE, Protocol}, {io.EOF, Protocol}, {io.ErrUnexpectedEOF, Protocol},
		{context.Canceled, Cancelled}, {syscall.ECANCELED, Cancelled},
		{errors.New("permission denied timeout protocol cancelled"), Other},
	}
	for _, tc := range cases {
		wrapped := fmt.Errorf("private wrapper: %w", &os.PathError{Op: "private", Path: "secret", Err: tc.err})
		if got := Classify(wrapped); got != tc.class {
			t.Fatalf("%T got %d want %d", tc.err, got, tc.class)
		}
		if !errors.Is(At(4, wrapped), tc.err) {
			t.Fatal("lost wrapped identity")
		}
	}
}

func TestContractBoundsAndExplicitClass(t *testing.T) {
	e := errors.New("opaque")
	if ExitCode(At(255, AsClass(255, e))) != 32 {
		t.Fatal("out of bounds")
	}
	if Classify(AsClass(Protocol, syscall.EINVAL)) != Protocol {
		t.Fatal("explicit invariant class lost")
	}
	if ExitCode(fmt.Errorf("outer: %w", At(7, syscall.EACCES))) != 89 {
		t.Fatal("typed stage lost")
	}
}
