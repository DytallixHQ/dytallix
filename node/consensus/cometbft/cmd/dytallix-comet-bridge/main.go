// This executable is a local qualification adapter, not a production endpoint.
package main

import (
	"bufio"
	"bytes"
	"context"
	ownerguard "dytallix.local/consensus/owner-guard"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"

	"dytallix.local/consensus/cometbft/internal/startupdiag"
	abciserver "github.com/cometbft/cometbft/abci/server"
	"github.com/cometbft/cometbft/libs/log"
)

const maxJSONBytes = 8 << 20
const callTimeout = 120 * time.Second

type responseEnvelope struct {
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result"`
	Error  string          `json:"error"`
}

type child struct {
	mu        sync.Mutex
	cmd       *exec.Cmd
	input     io.WriteCloser
	scan      *bufio.Scanner
	broken    error
	done      chan error
	output    io.ReadCloser // owned only in inherited mode
	closeOnce sync.Once
}

func startChild(ctx context.Context, args []string) (*child, error) {
	if len(args) == 0 {
		return nil, errors.New("supply the Rust child executable after --")
	}
	cmd := exec.CommandContext(ctx, args[0], args[1:]...)
	cmd.Stderr = os.Stderr
	in, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		_ = in.Close()
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	scan := bufio.NewScanner(out)
	scan.Buffer(make([]byte, 64<<10), maxJSONBytes)
	c := &child{cmd: cmd, input: in, scan: scan, done: make(chan error, 1)}
	go func() { c.done <- cmd.Wait() }()
	return c, nil
}

func (c *child) fail(err error) error {
	c.broken = err
	if c.cmd != nil {
		_ = c.cmd.Process.Kill()
	} else {
		c.closeInherited()
		select {
		case c.done <- err:
		default:
		}
	}
	return err
}

// ABCI uses several connections. A single mutex serializes the child protocol.
// A failed or timed-out exchange poisons the stream; it is never reused.
func (c *child) call(ctx context.Context, method string, payload, result any) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.broken != nil {
		return c.broken
	}
	req, err := json.Marshal(struct {
		Method  string `json:"method"`
		Payload any    `json:"payload"`
	}{method, payload})
	if err != nil {
		return err
	}
	if len(req)+1 > maxJSONBytes {
		return errors.New("child request exceeds local qualification limit")
	}
	ctx, cancel := context.WithTimeout(ctx, callTimeout)
	defer cancel()
	type exchange struct {
		data []byte
		err  error
	}
	ch := make(chan exchange, 1)
	go func() {
		if _, err := c.input.Write(append(req, '\n')); err != nil {
			ch <- exchange{err: err}
			return
		}
		if !c.scan.Scan() {
			err := c.scan.Err()
			if err == nil {
				err = io.ErrUnexpectedEOF
			}
			ch <- exchange{err: err}
			return
		}
		ch <- exchange{data: append([]byte(nil), c.scan.Bytes()...)}
	}()
	var got exchange
	select {
	case got = <-ch:
	case <-ctx.Done():
		return c.fail(fmt.Errorf("child %s: %w", method, ctx.Err()))
	}
	if got.err != nil {
		return c.fail(fmt.Errorf("child %s: %w", method, got.err))
	}
	var envelope responseEnvelope
	dec := json.NewDecoder(bytes.NewReader(got.data))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&envelope); err != nil {
		return c.fail(fmt.Errorf("child response: %w", err))
	}
	if err := dec.Decode(new(any)); err != io.EOF {
		return c.fail(errors.New("child response has trailing JSON"))
	}
	if !envelope.OK {
		if envelope.Error == "" {
			return c.fail(errors.New("child returned an unspecified error"))
		}
		return fmt.Errorf("application %s: %s", method, envelope.Error)
	}
	if envelope.Error != "" || len(envelope.Result) == 0 || string(envelope.Result) == "null" {
		return c.fail(errors.New("child returned an inconsistent response"))
	}
	if err := json.Unmarshal(envelope.Result, result); err != nil {
		return c.fail(fmt.Errorf("child result for %s: %w", method, err))
	}
	return nil
}

func privateSocketPath(raw string) (string, error) {
	if !strings.HasPrefix(raw, "unix://") {
		return "", errors.New("only unix:///absolute/private-directory/app.sock is allowed")
	}
	path := strings.TrimPrefix(raw, "unix://")
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return "", errors.New("socket path must be absolute and clean")
	}
	dir := filepath.Dir(path)
	info, err := os.Lstat(dir)
	if err != nil {
		return "", err
	}
	if !info.IsDir() || info.Mode().Perm() != 0o700 {
		return "", errors.New("socket parent must be a private directory with mode 0700")
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Getuid()) {
		return "", errors.New("socket parent must belong to this user")
	}
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		return "", errors.New("socket path already exists; do not overwrite an active or stale endpoint")
	}
	return path, nil
}

func run() error {
	flags := flag.NewFlagSet("dytallix-comet-bridge", flag.ContinueOnError)
	socket := flags.String("socket", "", "private unix:// ABCI socket")
	var mode, inputFD, outputFD singleFlag
	flags.Var(&mode, "application-channel", "explicit inherited-pipes-v1 application channel (Linux only)")
	flags.Var(&inputFD, "application-input-fd", "inherited write descriptor for application stdin")
	flags.Var(&outputFD, "application-output-fd", "inherited read descriptor for application stdout")
	if err := flags.Parse(os.Args[1:]); err != nil {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, err))
	}
	if !mode.set || mode.value != "inherited-pipes-v1" {
		return startupdiag.At(1, startupdiag.AsClass(startupdiag.Invalid, errors.New("guarded bridge requires inherited-pipes-v1; application spawning is unsupported")))
	}
	path, err := privateSocketPath(*socket)
	if err != nil {
		return startupdiag.At(2, err)
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	process, err := selectApplication(ctx, mode, inputFD, outputFD, flags.Args())
	if err != nil {
		return startupdiag.At(3, err)
	}
	if process.cmd != nil {
		defer process.cmd.Process.Kill()
	} else {
		defer process.closeInherited()
	}
	app := &application{child: process}
	server, err := abciserver.NewServer(*socket, "socket", app)
	if err != nil {
		return startupdiag.At(4, err)
	}
	server.SetLogger(log.NewTMLogger(log.NewSyncWriter(os.Stderr)))
	if err := server.Start(); err != nil {
		return startupdiag.At(5, err)
	}
	defer server.Stop()
	if err := os.Chmod(path, 0o600); err != nil {
		return startupdiag.At(6, err)
	}
	defer os.Remove(path)
	select {
	case <-ctx.Done():
		if process.cmd == nil {
			process.closeInherited()
		}
		return nil
	case err := <-process.done:
		if process.cmd == nil {
			return startupdiag.At(7, fmt.Errorf("inherited application channel failed: %w", err))
		}
		if err == nil {
			return startupdiag.At(7, errors.New("Rust application exited before bridge shutdown"))
		}
		return startupdiag.At(7, fmt.Errorf("Rust application exited: %w", err))
	}
}

func main() {
	if err := ownerguard.Run(ownerguard.Bridge, run); err != nil {
		os.Exit(startupdiag.ExitCode(err))
	}
}
