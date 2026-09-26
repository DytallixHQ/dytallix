// Package startupdiag defines bounded startup failure codes. It emits no output.
package startupdiag

import (
	"context"
	"errors"
	"io"
	"os"
	"syscall"
)

const Version = 1

type Class uint8

const (
	Other Class = iota
	Permission
	NotFound
	Invalid
	Timeout
	Resource
	Protocol
	Cancelled
)

type failure struct {
	stage uint8
	cause error
}

func (e *failure) Error() string { return e.cause.Error() }
func (e *failure) Unwrap() error { return e.cause }

// At retains the original error chain. Invalid stages fall back to guard/other.
func At(stage uint8, err error) error {
	if err == nil {
		return nil
	}
	if stage > 7 {
		stage = 0
	}
	return &failure{stage, err}
}

type classified struct {
	class Class
	cause error
}

func (e *classified) Error() string { return e.cause.Error() }
func (e *classified) Unwrap() error { return e.cause }
func AsClass(class Class, err error) error {
	if err == nil {
		return nil
	}
	if class > Cancelled {
		class = Other
	}
	return &classified{class, err}
}
func Classify(err error) Class {
	var c *classified
	if errors.As(err, &c) {
		return c.class
	}
	switch {
	case errors.Is(err, context.Canceled), errors.Is(err, syscall.ECANCELED):
		return Cancelled
	case errors.Is(err, context.DeadlineExceeded), errors.Is(err, os.ErrDeadlineExceeded), errors.Is(err, syscall.ETIMEDOUT):
		return Timeout
	case errors.Is(err, os.ErrPermission), errors.Is(err, syscall.EPERM):
		return Permission
	case errors.Is(err, os.ErrNotExist):
		return NotFound
	case errors.Is(err, os.ErrInvalid), errors.Is(err, syscall.EINVAL):
		return Invalid
	case errors.Is(err, syscall.ENOMEM), errors.Is(err, syscall.ENOSPC), errors.Is(err, syscall.EMFILE), errors.Is(err, syscall.ENFILE), errors.Is(err, syscall.EAGAIN):
		return Resource
	case errors.Is(err, syscall.EPROTO), errors.Is(err, syscall.EPIPE), errors.Is(err, io.EOF), errors.Is(err, io.ErrUnexpectedEOF):
		return Protocol
	}
	return Other
}

// ExitCode maps unwrapped owner-guard failures to stage zero. Nil is success.
func ExitCode(err error) int {
	if err == nil {
		return 0
	}
	var f *failure
	var stage uint8
	if errors.As(err, &f) {
		stage = f.stage
	}
	return 32 + 8*int(stage) + int(Classify(err))
}
