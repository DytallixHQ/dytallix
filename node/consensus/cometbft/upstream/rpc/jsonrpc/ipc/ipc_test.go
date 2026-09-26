package ipc

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/cometbft/cometbft/rpc/jsonrpc/dispatch"
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
)

func requestForTest(method, path, query string, body []byte) Request {
	return Request{Version: 1, Method: method, Path: path, Query: query, BodyBase64: base64.StdEncoding.EncodeToString(body), RemoteAddr: "127.0.0.1:4567"}
}
func requestBytes(t *testing.T, request Request) []byte {
	t.Helper()
	raw, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}
func privateDirectory(t *testing.T) string {
	t.Helper()
	path, err := os.MkdirTemp("/tmp", "dyt-ipc-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(path) })
	return path
}

type echoResult struct {
	Height int64  `json:"height"`
	Data   string `json:"data"`
	Remote string `json:"remote"`
}

func testRPC(t *testing.T) map[string]*dispatch.RPCFunc {
	return map[string]*dispatch.RPCFunc{"echo": dispatch.NewRPCFunc(func(ctx *types.Context, height int64, data []byte) (echoResult, error) {
		return echoResult{height, string(data), ctx.RemoteAddr()}, nil
	}, "height,data", dispatch.Cacheable("height")), "ws": dispatch.NewWSRPCFunc(func(*types.Context) (string, error) { return "ws", nil }, "")}
}
func bodyOf(t *testing.T, response Response) []byte {
	t.Helper()
	body, err := base64.StdEncoding.DecodeString(response.BodyBase64)
	if err != nil {
		t.Fatal(err)
	}
	return body
}
func TestNeutralDispatchPreservesGETAndPOST(t *testing.T) {
	handler := NewHandler(testRPC(t), 10)
	cases := []Request{requestForTest("GET", "/echo", "height=7&data=0x6162", nil), requestForTest("POST", "/", "", []byte(`{"jsonrpc":"2.0","id":"id7","method":"echo","params":{"height":"7","data":"YWI="}}`))}
	for _, request := range cases {
		_, body, err := decodeRequest(requestBytes(t, request))
		if err != nil {
			t.Fatal(err)
		}
		response := handler(context.Background(), request, body)
		if response.Status != 200 || response.Headers["Cache-Control"] != "public, max-age=86400" {
			t.Fatalf("response changed: %+v", response)
		}
		var wire types.RPCResponse
		if err = json.Unmarshal(bodyOf(t, response), &wire); err != nil {
			t.Fatal(err)
		}
		var result map[string]any
		if err = json.Unmarshal(wire.Result, &result); err != nil {
			t.Fatalf("result decode: %v; body=%s", err, bodyOf(t, response))
		}
		if result["height"] != "7" || result["data"] != "ab" || result["remote"] != request.RemoteAddr {
			t.Fatalf("RPC argument or context changed: %+v", result)
		}
		if request.Method == "GET" && wire.ID != types.JSONRPCIntID(-1) {
			t.Fatal("GET ID changed")
		}
		if request.Method == "POST" && wire.ID != types.JSONRPCStringID("id7") {
			t.Fatal("POST ID changed")
		}
	}
}
func TestNeutralDispatchErrorsNotificationsAndBatch(t *testing.T) {
	handler := NewHandler(testRPC(t), 1)
	cases := []struct {
		request Request
		status  int
		code    int
	}{
		{requestForTest("GET", "/missing", "", nil), 404, 0},
		{requestForTest("GET", "/websocket", "", nil), 501, 0},
		{requestForTest("POST", "/echo", "", []byte(`{}`)), 501, 0},
		{requestForTest("POST", "/", "", []byte(`{`)), 500, -32700},
		{requestForTest("POST", "/", "", []byte(`{"jsonrpc":"2.0","id":1,"method":"missing","params":{}}`)), 200, -32601},
		{requestForTest("POST", "/", "", []byte(`{"jsonrpc":"2.0","id":1,"method":"echo","params":{"height":"wrong"}}`)), 200, -32602},
		{requestForTest("POST", "/", "", []byte(`[{"jsonrpc":"2.0","id":1,"method":"echo"},{"jsonrpc":"2.0","id":2,"method":"echo"}]`)), 400, 0},
	}
	for _, test := range cases {
		_, body, err := decodeRequest(requestBytes(t, test.request))
		if err != nil {
			t.Fatal(err)
		}
		response := handler(context.Background(), test.request, body)
		if response.Status != test.status {
			t.Fatalf("status=%d want=%d", response.Status, test.status)
		}
		if test.code != 0 {
			var wire types.RPCResponse
			_ = json.Unmarshal(bodyOf(t, response), &wire)
			if wire.Error == nil || wire.Error.Code != test.code {
				t.Fatalf("RPC error changed: %+v", wire)
			}
		}
	}
	request := requestForTest("POST", "/", "", []byte(`{"jsonrpc":"2.0","method":"echo","params":{}}`))
	_, body, _ := decodeRequest(requestBytes(t, request))
	if response := handler(context.Background(), request, body); len(bodyOf(t, response)) != 0 {
		t.Fatal("notification produced response")
	}
}
func TestIPCRequestRejectsNoncanonicalMetadata(t *testing.T) {
	valid := string(requestBytes(t, requestForTest("GET", "/echo", "", nil)))
	cases := []string{
		strings.Replace(valid, `"version":1`, `"version":1,"version":1`, 1),
		strings.Replace(valid, `"version":1`, `"version":1.0`, 1),
		strings.Replace(valid, `"query":""`, `"query":null`, 1),
		strings.Replace(valid, `"query":""`, `"unknown":""`, 1),
		strings.Replace(valid, `127.0.0.1:4567`, `192.0.2.1:4567`, 1),
		strings.Replace(valid, `/echo`, `/../echo`, 1),
		strings.Replace(valid, `"body_base64":""`, `"body_base64":"YQ"`, 1),
		valid + `{}`,
	}
	for i, raw := range cases {
		if _, _, err := decodeRequest([]byte(raw)); err == nil {
			t.Fatalf("invalid metadata accepted:%d", i)
		}
	}
	if _, _, err := decodeRequest([]byte(valid)); err != nil {
		t.Fatal(err)
	}
}
func TestIPCFrameAndBodyLimits(t *testing.T) {
	var prefix [4]byte
	binary.BigEndian.PutUint32(prefix[:], MaxFrameBytes+1)
	if _, err := readFrame(bytes.NewReader(prefix[:])); err == nil {
		t.Fatal("oversized frame accepted")
	}
	request := requestForTest("POST", "/", "", bytes.Repeat([]byte("a"), MaxRequestBodyBytes+1))
	if _, _, err := decodeRequest(requestBytes(t, request)); err == nil {
		t.Fatal("oversized request body accepted")
	}
	if response := responseBody(200, make([]byte, MaxResponseBodyBytes+1), false); response.Status != 507 {
		t.Fatal("oversized response was not rejected")
	}
	raw := []byte("frame")
	var buffer bytes.Buffer
	if err := writeFrame(&buffer, raw); err != nil {
		t.Fatal(err)
	}
	decoded, err := readFrame(&buffer)
	if err != nil || !bytes.Equal(raw, decoded) {
		t.Fatal("frame roundtrip failed")
	}
}
func TestIPCPrivateSocketRoundTripAndCleanup(t *testing.T) {
	path := filepath.Join(privateDirectory(t), "rpc.sock")
	server, err := Listen(path, NewHandler(testRPC(t), 10))
	if err != nil {
		t.Fatal(err)
	}
	stat, err := os.Stat(path)
	if err != nil || stat.Mode().Perm() != 0600 {
		t.Fatal("socket mode changed")
	}
	connection, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	_ = connection.SetDeadline(time.Now().Add(2 * time.Second))
	request := requestForTest("GET", "/echo", "height=7&data=0x6162", nil)
	if err = writeFrame(connection, requestBytes(t, request)); err != nil {
		t.Fatal(err)
	}
	frame, err := readFrame(connection)
	if err != nil {
		t.Fatal(err)
	}
	var response Response
	if err = json.Unmarshal(frame, &response); err != nil || response.Status != 200 {
		t.Fatalf("bad socket response:%v", err)
	}
	var next [1]byte
	if _, err = connection.Read(next[:]); err != io.EOF {
		t.Fatalf("connection was not closed after one response:%v", err)
	}
	_ = connection.Close()
	_ = server.Close()
	if _, err = os.Lstat(path); !os.IsNotExist(err) {
		t.Fatal("owned socket remains after close")
	}
}
func TestIPCRefusesExistingPathAndPublicDirectory(t *testing.T) {
	directory := privateDirectory(t)
	path := filepath.Join(directory, "rpc.sock")
	if err := os.WriteFile(path, []byte("retain"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Listen(path, NewHandler(testRPC(t), 10)); err == nil {
		t.Fatal("existing path replaced")
	}
	raw, _ := os.ReadFile(path)
	if string(raw) != "retain" {
		t.Fatal("existing file changed")
	}
	_ = os.Remove(path)
	_ = os.Chmod(directory, 0755)
	if _, err := Listen(path, NewHandler(testRPC(t), 10)); err == nil {
		t.Fatal("public socket parent accepted")
	}
}
func TestIPCDisconnectCancelsAndWorkerLimitHolds(t *testing.T) {
	started := make(chan struct{}, MaxConnections)
	canceled := make(chan struct{}, MaxConnections)
	handler := func(ctx context.Context, _ Request, _ []byte) Response {
		started <- struct{}{}
		<-ctx.Done()
		canceled <- struct{}{}
		return errorResponse(504, "canceled")
	}
	path := filepath.Join(privateDirectory(t), "rpc.sock")
	server, err := Listen(path, handler)
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	connections := make([]net.Conn, 0, MaxConnections)
	for i := 0; i < MaxConnections; i++ {
		c, err := net.Dial("unix", path)
		if err != nil {
			t.Fatal(err)
		}
		connections = append(connections, c)
		if err = writeFrame(c, requestBytes(t, requestForTest("GET", "/echo", "", nil))); err != nil {
			t.Fatal(err)
		}
	}
	for i := 0; i < MaxConnections; i++ {
		select {
		case <-started:
		case <-time.After(2 * time.Second):
			t.Fatal("workers did not start")
		}
	}
	extra, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	_ = extra.SetDeadline(time.Now().Add(time.Second))
	var data [1]byte
	if _, err = extra.Read(data[:]); err != io.EOF {
		t.Fatalf("excess connection not closed:%v", err)
	}
	_ = extra.Close()
	for _, connection := range connections {
		_ = connection.Close()
	}
	for i := 0; i < MaxConnections; i++ {
		select {
		case <-canceled:
		case <-time.After(2 * time.Second):
			t.Fatal("disconnect did not cancel RPC")
		}
	}
}
func TestRPCRequestContextDefaultsAndCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	request := types.NewRequestContext(ctx, "127.0.0.1:1", nil)
	cancel()
	if request.Context().Err() != context.Canceled || request.RemoteAddr() != "127.0.0.1:1" {
		t.Fatal("neutral context lost cancellation or remote")
	}
	empty := types.NewRequestContext(nil, "", nil)
	if empty.Context() == nil || !reflect.DeepEqual(empty.RemoteAddr(), "") {
		t.Fatal("nil context default changed")
	}
}
