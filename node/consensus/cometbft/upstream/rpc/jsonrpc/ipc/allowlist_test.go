package ipc_test

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/cometbft/cometbft/rpc/core"
	"github.com/cometbft/cometbft/rpc/jsonrpc/ipc"
)

// The allowlists name only real routes; the client socket excludes every
// search, mempool, commit-waiting, WebSocket and diagnostic method.
func TestAllowlistsSelectExactlyTheApprovedRoutes(t *testing.T) {
	routes := (&core.Environment{}).GetRoutes()
	client, err := ipc.Select(routes, ipc.ClientMethods)
	if err != nil {
		t.Fatal(err)
	}
	operator, err := ipc.Select(routes, ipc.ClientMethods, ipc.OperatorMethods)
	if err != nil {
		t.Fatal(err)
	}
	if len(client) != len(ipc.ClientMethods) || len(operator) != len(ipc.ClientMethods)+len(ipc.OperatorMethods) {
		t.Fatalf("client %d, operator %d", len(client), len(operator))
	}
	for _, name := range []string{"tx_search", "block_search", "unconfirmed_txs", "broadcast_tx_commit",
		"broadcast_tx_async", "subscribe", "unsubscribe", "unsubscribe_all", "genesis"} {
		if _, ok := operator[name]; ok {
			t.Fatalf("%s is served", name)
		}
	}
	for _, name := range ipc.OperatorMethods {
		if _, ok := client[name]; ok {
			t.Fatalf("operator method %s is on the client socket", name)
		}
	}
	if _, err = ipc.Select(routes, []string{"status", "no_such_method"}); err == nil ||
		!strings.Contains(err.Error(), "no_such_method") {
		t.Fatalf("a missing route must be an error: %v", err)
	}
}

// A method outside the allowlist is not found, in both request forms.
func TestExcludedMethodsAreNotFound(t *testing.T) {
	client, err := ipc.Select((&core.Environment{}).GetRoutes(), ipc.ClientMethods)
	if err != nil {
		t.Fatal(err)
	}
	handler := ipc.NewHandler(client, 10)
	get := handler(context.Background(), ipc.Request{Version: 1, Method: "GET", Path: "/net_info", RemoteAddr: "127.0.0.1:1"}, nil)
	if get.Status != 404 {
		t.Fatalf("GET status %d", get.Status)
	}
	body := []byte(`{"jsonrpc":"2.0","id":1,"method":"tx_search","params":{"query":"tx.height=1"}}`)
	post := handler(context.Background(), ipc.Request{Version: 1, Method: "POST", Path: "/",
		BodyBase64: base64.StdEncoding.EncodeToString(body), RemoteAddr: "127.0.0.1:1"}, body)
	raw, err := base64.StdEncoding.DecodeString(post.BodyBase64)
	if err != nil {
		t.Fatal(err)
	}
	var reply []struct {
		Error struct {
			Code int `json:"code"`
		} `json:"error"`
	}
	if err = json.Unmarshal(raw, &reply); err != nil {
		var single struct {
			Error struct {
				Code int `json:"code"`
			} `json:"error"`
		}
		if err = json.Unmarshal(raw, &single); err != nil {
			t.Fatalf("reply %s: %v", raw, err)
		}
		reply = append(reply, single)
	}
	if len(reply) != 1 || reply[0].Error.Code != -32601 {
		t.Fatalf("tx_search reply %s", raw)
	}
}

// get sends one framed GET over a socket and returns the response status.
func get(t *testing.T, socket, method string) int {
	t.Helper()
	conn, err := net.Dial("unix", socket)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	request, err := json.Marshal(ipc.Request{Version: 1, Method: "GET", Path: "/" + method, RemoteAddr: "127.0.0.1:1"})
	if err != nil {
		t.Fatal(err)
	}
	frame := make([]byte, 4, 4+len(request))
	binary.BigEndian.PutUint32(frame, uint32(len(request)))
	if _, err = conn.Write(append(frame, request...)); err != nil {
		t.Fatal(err)
	}
	var size [4]byte
	if _, err = io.ReadFull(conn, size[:]); err != nil {
		t.Fatal(err)
	}
	raw := make([]byte, binary.BigEndian.Uint32(size[:]))
	if _, err = io.ReadFull(conn, raw); err != nil {
		t.Fatal(err)
	}
	var response ipc.Response
	if err = json.Unmarshal(raw, &response); err != nil {
		t.Fatal(err)
	}
	return response.Status
}

// Both sockets are private, and only the operator socket serves diagnostics.
// A found method that fails (its environment is empty here) answers 500; an
// excluded one answers 404.
func TestSocketsServeTheirAllowlistsPrivately(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "dyt-rpc-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)
	if err = os.Chmod(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	listeners, err := ipc.ServeSockets(dir, (&core.Environment{}).GetRoutes(), 10)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		for _, l := range listeners {
			_ = l.Close()
		}
	}()
	for _, name := range []string{ipc.ClientSocket, ipc.OperatorSocket} {
		info, err := os.Stat(filepath.Join(dir, name))
		if err != nil || info.Mode()&os.ModeSocket == 0 || info.Mode().Perm() != 0o600 {
			t.Fatalf("%s: %v %v", name, info, err)
		}
	}
	client, operator := filepath.Join(dir, ipc.ClientSocket), filepath.Join(dir, ipc.OperatorSocket)
	if status := get(t, client, "net_info"); status != 404 {
		t.Fatalf("client net_info %d", status)
	}
	if status := get(t, operator, "net_info"); status == 404 {
		t.Fatal("operator net_info not found")
	}
	if status := get(t, operator, "tx_search"); status != 404 {
		t.Fatalf("operator tx_search %d", status)
	}
	if status := get(t, client, "health"); status == 404 {
		t.Fatal("client health not found")
	}
	// A second start over existing sockets is refused.
	if _, err = ipc.ServeSockets(dir, (&core.Environment{}).GetRoutes(), 10); err == nil {
		t.Fatal("existing sockets must be refused")
	}
}
