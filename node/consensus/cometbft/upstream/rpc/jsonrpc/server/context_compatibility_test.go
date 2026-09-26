package server

import (
	"context"
	"encoding/json"
	"github.com/cometbft/cometbft/libs/log"
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

type contextTestKey struct{}
type contextResult struct {
	Remote   string `json:"remote"`
	Value    string `json:"value"`
	Canceled bool   `json:"canceled"`
	Deadline bool   `json:"deadline"`
	Argument string `json:"argument"`
}

func TestTransportContextCompatibility(t *testing.T) {
	for _, method := range []string{"GET", "POST"} {
		t.Run(method, func(t *testing.T) {
			call := NewRPCFunc(func(ctx *types.Context, value string) (*contextResult, error) {
				_, deadline := ctx.Context().Deadline()
				return &contextResult{Remote: ctx.RemoteAddr(), Value: ctx.Context().Value(contextTestKey{}).(string), Canceled: ctx.Context().Err() != nil, Deadline: deadline, Argument: value}, nil
			}, "value")
			mux := http.NewServeMux()
			RegisterRPCFuncs(mux, map[string]*RPCFunc{"context": call}, log.NewNopLogger())
			path := "/context?value=%22hello%22"
			body := ""
			if method == "POST" {
				path = "/"
				body = `{"jsonrpc":"2.0","id":"context-id","method":"context","params":{"value":"hello"}}`
			}
			ctx, cancel := context.WithTimeout(context.WithValue(context.Background(), contextTestKey{}, "retained"), time.Minute)
			cancel()
			req := httptest.NewRequest(method, path, strings.NewReader(body)).WithContext(ctx)
			req.RemoteAddr = "127.0.0.1:4567"
			response := httptest.NewRecorder()
			mux.ServeHTTP(response, req)
			if response.Code != 200 {
				t.Fatalf("HTTP status changed: %d", response.Code)
			}
			var wire types.RPCResponse
			if err := json.Unmarshal(response.Body.Bytes(), &wire); err != nil {
				t.Fatal(err)
			}
			if wire.Error != nil {
				t.Fatal(wire.Error)
			}
			var got contextResult
			if err := json.Unmarshal(wire.Result, &got); err != nil {
				t.Fatal(err)
			}
			want := contextResult{Remote: req.RemoteAddr, Value: "retained", Canceled: true, Deadline: true, Argument: "hello"}
			if got != want {
				t.Fatalf("request context changed: %+v", got)
			}
			if method == "POST" && wire.ID != types.JSONRPCStringID("context-id") {
				t.Fatal("request ID changed")
			}
		})
	}
}
