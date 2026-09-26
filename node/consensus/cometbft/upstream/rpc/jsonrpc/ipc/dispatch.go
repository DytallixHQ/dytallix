package ipc

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"reflect"
	"strings"

	"github.com/cometbft/cometbft/rpc/jsonrpc/dispatch"
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
)

type Handler func(context.Context, Request, []byte) Response

// NewHandler preserves method registration and parameter conversion from the existing RPC adapters.
// HTTP and WebSocket parsing remain outside this package.
func NewHandler(routes map[string]*dispatch.RPCFunc, maxBatch int) Handler {
	if maxBatch < 1 || maxBatch > 10 {
		maxBatch = 10
	}
	return func(ctx context.Context, request Request, body []byte) (response Response) {
		defer func() {
			if recover() != nil {
				response = rpcResponse(500, false, types.RPCInternalError(nil, errors.New("RPC method failed")))
			}
		}()
		if request.Path == "/websocket" {
			return errorResponse(501, "WebSocket is not supported by this experimental RPC profile")
		}
		if request.Method == "GET" {
			if request.Path == "/" {
				return errorResponse(501, "root browsing is not supported by this experimental RPC profile")
			}
			method := routes[strings.TrimPrefix(request.Path, "/")]
			id := types.JSONRPCIntID(-1)
			if method == nil {
				return errorResponse(404, "RPC method not found")
			}
			if method.WebSocketOnly() {
				return errorResponse(501, "WebSocket is not supported by this experimental RPC profile")
			}
			query, err := url.ParseQuery(request.Query)
			if err != nil {
				return rpcResponse(500, false, types.RPCInvalidParamsError(id, err))
			}
			args, err := dispatch.URIParamsToArgs(method, query.Get)
			if err != nil {
				return rpcResponse(500, false, types.RPCInvalidParamsError(id, fmt.Errorf("error converting http params to arguments: %w", err)))
			}
			args = append([]reflect.Value{reflect.ValueOf(types.NewRequestContext(ctx, request.RemoteAddr, nil))}, args...)
			result, err := call(method, args)
			if err != nil {
				return rpcResponse(500, false, types.RPCInternalError(id, err))
			}
			return rpcResponse(200, method.CacheableWithArgs(args), types.NewRPCSuccessResponse(id, result))
		}
		if request.Path != "/" {
			return errorResponse(501, "URI-form POST is not supported by this experimental RPC profile")
		}
		if len(body) == 0 {
			return errorResponse(501, "root browsing is not supported by this experimental RPC profile")
		}
		var requests []types.RPCRequest
		if err := json.Unmarshal(body, &requests); err != nil {
			var single types.RPCRequest
			if err = json.Unmarshal(body, &single); err != nil {
				return rpcResponse(500, false, types.RPCParseError(fmt.Errorf("error unmarshaling request: %w", err)))
			}
			requests = []types.RPCRequest{single}
		}
		if len(requests) > maxBatch {
			return errorResponse(400, "JSON-RPC batch exceeds experimental profile limit")
		}
		responses := make([]types.RPCResponse, 0, len(requests))
		cache := true
		for _, rpcRequest := range requests {
			// Preserve the existing Comet HTTP behavior: notifications do not invoke a method.
			if rpcRequest.ID == nil {
				continue
			}
			if ctx.Err() != nil {
				return errorResponse(504, "RPC request canceled or timed out")
			}
			method := routes[rpcRequest.Method]
			if method == nil || method.WebSocketOnly() {
				responses = append(responses, types.RPCMethodNotFoundError(rpcRequest.ID))
				cache = false
				continue
			}
			params := rpcRequest.Params
			if len(params) == 0 {
				params = []byte("null")
			}
			args, err := dispatch.JSONParamsToArgs(method, params)
			if err != nil {
				responses = append(responses, types.RPCInvalidParamsError(rpcRequest.ID, fmt.Errorf("error converting json params to arguments: %w", err)))
				cache = false
				continue
			}
			args = append([]reflect.Value{reflect.ValueOf(types.NewRequestContext(ctx, request.RemoteAddr, &rpcRequest))}, args...)
			cache = cache && method.CacheableWithArgs(args)
			result, err := call(method, args)
			if err != nil {
				responses = append(responses, types.RPCInternalError(rpcRequest.ID, err))
				continue
			}
			responses = append(responses, types.NewRPCSuccessResponse(rpcRequest.ID, result))
		}
		if len(responses) == 0 {
			return responseBody(200, nil, false)
		}
		return rpcResponse(200, cache, responses...)
	}
}
func call(method *dispatch.RPCFunc, args []reflect.Value) (any, error) {
	returns := method.Call(args)
	if returns[1].Interface() != nil {
		return nil, fmt.Errorf("%v", returns[1].Interface())
	}
	value := returns[0]
	result := reflect.New(value.Type())
	result.Elem().Set(value)
	return result.Interface(), nil
}
func rpcResponse(status int, cache bool, responses ...types.RPCResponse) Response {
	var value any = responses
	if len(responses) == 1 {
		value = responses[0]
	}
	raw, err := json.Marshal(value)
	if err != nil {
		return errorResponse(500, "RPC response encoding failed")
	}
	return responseBody(status, raw, cache)
}
