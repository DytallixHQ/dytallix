// Read-only diagnostics from the engine's operator socket (E04 gap 17, T-b;
// P01 28 September 2026). The socket, HOME/data/rpc-operator.sock, is for
// the node's owner (RPC controls v1); the HTTP adapter never forwards it.
//
//	dytallix-operator-rpc --home HOME METHOD [--query 'height=5']
//
// METHOD is one of net_info, consensus_state, dump_consensus_state,
// num_unconfirmed_txs, status, health or validators. It prints the engine's
// JSON-RPC response and submits nothing.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/cometbft/cometbft/rpc/jsonrpc/ipc"
)

// methods the client sends; none changes node state.
var methods = map[string]bool{
	"net_info": true, "consensus_state": true, "dump_consensus_state": true,
	"num_unconfirmed_txs": true, "status": true, "health": true, "validators": true,
}

func usage() error {
	return errors.New("usage: dytallix-operator-rpc --home HOME METHOD [--query QUERY]; METHOD is one of " +
		"net_info, consensus_state, dump_consensus_state, num_unconfirmed_txs, status, health, validators")
}

func frame(data []byte) []byte {
	out := make([]byte, 4, 4+len(data))
	binary.BigEndian.PutUint32(out, uint32(len(data)))
	return append(out, data...)
}

func readFrame(reader io.Reader) ([]byte, error) {
	var prefix [4]byte
	if _, err := io.ReadFull(reader, prefix[:]); err != nil {
		return nil, err
	}
	length := binary.BigEndian.Uint32(prefix[:])
	if length == 0 || length > ipc.MaxFrameBytes {
		return nil, errors.New("invalid response frame length")
	}
	data := make([]byte, length)
	_, err := io.ReadFull(reader, data)
	return data, err
}

// call sends one request and returns the status and body of its response.
func call(socket, method, query string) (int, []byte, error) {
	conn, err := net.DialTimeout("unix", socket, 5*time.Second)
	if err != nil {
		return 0, nil, fmt.Errorf("cannot reach the operator socket %s: %w", socket, err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(ipc.RequestTimeout + 5*time.Second))
	request, err := json.Marshal(ipc.Request{Version: 1, Method: "GET", Path: "/" + method, Query: query,
		BodyBase64: "", RemoteAddr: "127.0.0.1:1"})
	if err != nil {
		return 0, nil, err
	}
	// The write side stays open: closing it would cancel the call.
	if _, err = conn.Write(frame(request)); err != nil {
		return 0, nil, err
	}
	raw, err := readFrame(conn)
	if err != nil {
		return 0, nil, err
	}
	var response ipc.Response
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&response); err != nil || response.Version != 1 {
		return 0, nil, errors.New("invalid operator socket response")
	}
	body, err := base64.StdEncoding.Strict().DecodeString(response.BodyBase64)
	if err != nil || len(body) > ipc.MaxResponseBodyBytes {
		return 0, nil, errors.New("invalid operator socket response body")
	}
	return response.Status, body, nil
}

func run(args []string, out io.Writer) error {
	flags := flag.NewFlagSet("dytallix-operator-rpc", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	home := flags.String("home", "", "engine home")
	query := flags.String("query", "", "URI query, for example height=5")
	// The method may come before or after the flags.
	var method string
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		method, args = args[0], args[1:]
	}
	if err := flags.Parse(args); err != nil {
		return usage()
	}
	if method == "" && flags.NArg() == 1 {
		method = flags.Arg(0)
	} else if flags.NArg() != 0 {
		return usage()
	}
	if *home == "" || !filepath.IsAbs(*home) || !methods[method] {
		return usage()
	}
	if len(*query) > 16_384 || strings.ContainsAny(*query, "\x00\r\n#") {
		return errors.New("invalid query")
	}
	socket := filepath.Join(filepath.Clean(*home), "data", ipc.OperatorSocket)
	status, body, err := call(socket, method, *query)
	if err != nil {
		return err
	}
	if _, err = out.Write(append(body, '\n')); err != nil {
		return err
	}
	if status != 200 {
		return fmt.Errorf("the engine answered HTTP status %d", status)
	}
	return nil
}

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
