// Package ipc carries RPC requests over a private Unix socket. It does not parse HTTP.
package ipc

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

const MaxFrameBytes = 2_097_152
const MaxRequestBodyBytes = 1_048_576
const MaxResponseBodyBytes = 1_500_000
const MaxConnections = 32
const RequestTimeout = 10 * time.Second

var rpcPath = regexp.MustCompile(`^/(?:[A-Za-z_][A-Za-z_0-9]*)?$`)

type Request struct {
	Version    int    `json:"version"`
	Method     string `json:"method"`
	Path       string `json:"path"`
	Query      string `json:"query"`
	BodyBase64 string `json:"body_base64"`
	RemoteAddr string `json:"remote_addr"`
}
type Response struct {
	Version    int               `json:"version"`
	Status     int               `json:"status"`
	Headers    map[string]string `json:"headers"`
	BodyBase64 string            `json:"body_base64"`
}

func decodeRequest(raw []byte) (Request, []byte, error) {
	var request Request
	if !utf8.Valid(raw) {
		return request, nil, errors.New("request is not UTF-8")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return request, nil, errors.New("request must be an object")
	}
	required := map[string]bool{"version": false, "method": false, "path": false, "query": false, "body_base64": false, "remote_addr": false}
	fields := make(map[string]json.RawMessage, len(required))
	for decoder.More() {
		token, err = decoder.Token()
		if err != nil {
			return request, nil, err
		}
		name, ok := token.(string)
		if !ok {
			return request, nil, errors.New("invalid request field")
		}
		used, known := required[name]
		if !known || used {
			return request, nil, errors.New("unknown or duplicate request field")
		}
		required[name] = true
		var value json.RawMessage
		if err = decoder.Decode(&value); err != nil {
			return request, nil, err
		}
		fields[name] = value
	}
	if _, err = decoder.Token(); err != nil {
		return request, nil, err
	}
	if _, err = decoder.Token(); err != io.EOF {
		return request, nil, errors.New("trailing request data")
	}
	for _, present := range required {
		if !present {
			return request, nil, errors.New("missing request field")
		}
	}
	for name, value := range fields {
		if name != "version" && (len(value) == 0 || value[0] != '"') {
			return request, nil, errors.New("request metadata must contain strings")
		}
	}
	if string(fields["version"]) != "1" {
		return request, nil, errors.New("unsupported request version")
	}
	if err = json.Unmarshal(raw, &request); err != nil {
		return request, nil, err
	}
	if request.Method != "GET" && request.Method != "POST" {
		return request, nil, errors.New("unsupported method")
	}
	if len(request.Path) > 128 || !rpcPath.MatchString(request.Path) {
		return request, nil, errors.New("noncanonical RPC path")
	}
	if len(request.Query) > 16_384 || strings.ContainsAny(request.Query, "\x00\r\n#") {
		return request, nil, errors.New("invalid query")
	}
	host, port, err := net.SplitHostPort(request.RemoteAddr)
	if err != nil {
		return request, nil, errors.New("invalid remote address")
	}
	ip := net.ParseIP(host)
	number, err := strconv.Atoi(port)
	if ip == nil || !ip.IsLoopback() || err != nil || number < 1 || number > 65535 || strconv.Itoa(number) != port {
		return request, nil, errors.New("nonlocal remote address")
	}
	if len(request.BodyBase64) > base64.StdEncoding.EncodedLen(MaxRequestBodyBytes) {
		return request, nil, errors.New("request body exceeds limit")
	}
	body, err := base64.StdEncoding.Strict().DecodeString(request.BodyBase64)
	if err != nil || len(body) > MaxRequestBodyBytes || base64.StdEncoding.EncodeToString(body) != request.BodyBase64 {
		return request, nil, errors.New("invalid or oversized body encoding")
	}
	if request.Method == "GET" && len(body) != 0 {
		return request, nil, errors.New("GET body is not supported")
	}
	return request, body, nil
}

func readFrame(reader io.Reader) ([]byte, error) {
	var prefix [4]byte
	if _, err := io.ReadFull(reader, prefix[:]); err != nil {
		return nil, err
	}
	length := binary.BigEndian.Uint32(prefix[:])
	if length == 0 || length > MaxFrameBytes {
		return nil, errors.New("invalid frame length")
	}
	data := make([]byte, int(length))
	_, err := io.ReadFull(reader, data)
	return data, err
}
func writeFrame(writer io.Writer, data []byte) error {
	if len(data) == 0 || len(data) > MaxFrameBytes {
		return errors.New("invalid response frame length")
	}
	var prefix [4]byte
	binary.BigEndian.PutUint32(prefix[:], uint32(len(data)))
	if _, err := io.Copy(writer, bytes.NewReader(prefix[:])); err != nil {
		return err
	}
	_, err := io.Copy(writer, bytes.NewReader(data))
	return err
}
func responseBody(status int, body []byte, cache bool) Response {
	if len(body) > MaxResponseBodyBytes {
		return errorResponse(507, "RPC response exceeds experimental profile limit")
	}
	headers := map[string]string{"Content-Type": "application/json"}
	if cache {
		headers["Cache-Control"] = "public, max-age=86400"
	}
	return Response{Version: 1, Status: status, Headers: headers, BodyBase64: base64.StdEncoding.EncodeToString(body)}
}
func errorResponse(status int, message string) Response {
	raw, _ := json.Marshal(map[string]string{"error": message})
	return Response{Version: 1, Status: status, Headers: map[string]string{"Content-Type": "application/json"}, BodyBase64: base64.StdEncoding.EncodeToString(raw)}
}
func marshalResponse(response Response) ([]byte, error) {
	if response.Version != 1 || response.Status < 100 || response.Status > 599 {
		return nil, errors.New("invalid response metadata")
	}
	body, err := base64.StdEncoding.Strict().DecodeString(response.BodyBase64)
	if err != nil || len(body) > MaxResponseBodyBytes || base64.StdEncoding.EncodeToString(body) != response.BodyBase64 {
		return nil, errors.New("invalid response body")
	}
	for name, value := range response.Headers {
		if name != "Content-Type" && name != "Cache-Control" {
			return nil, errors.New("unsupported response header")
		}
		if strings.ContainsAny(value, "\r\n\x00") || len(value) > 256 {
			return nil, errors.New("invalid response header")
		}
	}
	raw, err := json.Marshal(response)
	if err != nil {
		return nil, err
	}
	if len(raw) > MaxFrameBytes {
		return nil, fmt.Errorf("response frame exceeds %d bytes", MaxFrameBytes)
	}
	return raw, nil
}
