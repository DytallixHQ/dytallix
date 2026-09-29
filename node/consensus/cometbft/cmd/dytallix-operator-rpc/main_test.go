package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/cometbft/cometbft/rpc/jsonrpc/ipc"
)

// home starts a real operator socket whose handler echoes the request.
func home(t *testing.T, status int) string {
	// A short path: Unix socket paths are limited to about 100 bytes.
	dir, err := os.MkdirTemp("/tmp", "dyt-op")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	data := filepath.Join(dir, "data")
	if err = os.Mkdir(data, 0o700); err != nil {
		t.Fatal(err)
	}
	server, err := ipc.Listen(filepath.Join(data, ipc.OperatorSocket), func(_ context.Context, request ipc.Request, _ []byte) ipc.Response {
		body, _ := json.Marshal(map[string]string{"method": request.Method, "path": request.Path, "query": request.Query})
		return ipc.Response{Version: 1, Status: status, Headers: map[string]string{"Content-Type": "application/json"},
			BodyBase64: base64.StdEncoding.EncodeToString(body)}
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	return dir
}

func TestReadsADiagnosticFromTheOperatorSocket(t *testing.T) {
	dir := home(t, 200)
	var out bytes.Buffer
	if err := run([]string{"validators", "--home", dir, "--query", "height=5"}, &out); err != nil {
		t.Fatal(err)
	}
	var echoed map[string]string
	if err := json.Unmarshal(out.Bytes(), &echoed); err != nil {
		t.Fatal(err)
	}
	if echoed["method"] != "GET" || echoed["path"] != "/validators" || echoed["query"] != "height=5" {
		t.Fatalf("unexpected request %v", echoed)
	}
	out.Reset()
	if err := run([]string{"--home", dir, "net_info"}, &out); err != nil || !strings.Contains(out.String(), "/net_info") {
		t.Fatalf("method after flags: %v %s", err, out.String())
	}
}

func TestSubmitsNothingAndReportsFailures(t *testing.T) {
	dir := home(t, 500)
	for _, args := range [][]string{
		{"broadcast_tx_sync", "--home", dir},
		{"broadcast_evidence", "--home", dir},
		{"status", "--home", "relative/home"},
		{"status"},
		{"status", "extra", "--home", dir},
		{"status", "--home", dir, "--query", "a=1\nb=2"},
	} {
		if err := run(args, &bytes.Buffer{}); err == nil {
			t.Fatalf("accepted %v", args)
		}
	}
	var out bytes.Buffer
	if err := run([]string{"status", "--home", dir}, &out); err == nil || !strings.Contains(out.String(), "/status") {
		t.Fatalf("a failed status must still print the body: %v %s", err, out.String())
	}
	if err := run([]string{"status", "--home", filepath.Join(dir, "missing")}, &bytes.Buffer{}); err == nil {
		t.Fatal("reached a missing socket")
	}
}
