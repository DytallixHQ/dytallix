package server

import (
	"bytes"
	"encoding/json"
	"fmt"
	"html"
	"io"
	"net/http"
	"reflect"
	"sort"

	"github.com/cometbft/cometbft/libs/log"
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
)

// HTTP + JSON handler

// jsonrpc calls grab the given method's function info and runs reflect.Call
func makeJSONRPCHandler(funcMap map[string]*RPCFunc, logger log.Logger) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		b, err := io.ReadAll(r.Body)
		if err != nil {
			res := types.RPCInvalidRequestError(nil,
				fmt.Errorf("error reading request body: %w", err),
			)
			if wErr := WriteRPCResponseHTTPError(w, http.StatusBadRequest, res); wErr != nil {
				logger.Error("failed to write response", "err", wErr)
			}
			return
		}

		// if it's an empty request (like from a browser), just display a list of
		// functions
		if len(b) == 0 {
			writeListOfEndpoints(w, r, funcMap)
			return
		}

		// first try to unmarshal the incoming request as an array of RPC requests
		var (
			requests  []types.RPCRequest
			responses []types.RPCResponse
		)
		if err := json.Unmarshal(b, &requests); err != nil {
			// next, try to unmarshal as a single request
			var request types.RPCRequest
			if err := json.Unmarshal(b, &request); err != nil {
				res := types.RPCParseError(fmt.Errorf("error unmarshaling request: %w", err))
				if wErr := WriteRPCResponseHTTPError(w, http.StatusInternalServerError, res); wErr != nil {
					logger.Error("failed to write response", "err", wErr)
				}
				return
			}
			requests = []types.RPCRequest{request}
		}

		// Set the default response cache to true unless
		// 1. Any RPC request error.
		// 2. Any RPC request doesn't allow to be cached.
		// 3. Any RPC request has the height argument and the value is 0 (the default).
		cache := true
		for _, request := range requests {

			// A Notification is a Request object without an "id" member.
			// The Server MUST NOT reply to a Notification, including those that are within a batch request.
			if request.ID == nil {
				logger.Debug(
					"HTTPJSONRPC received a notification, skipping... (please send a non-empty ID if you want to call a method)",
					"req", request,
				)
				continue
			}
			if len(r.URL.Path) > 1 {
				responses = append(
					responses,
					types.RPCInvalidRequestError(request.ID, fmt.Errorf("path %s is invalid", r.URL.Path)),
				)
				cache = false
				continue
			}
			rpcFunc, ok := funcMap[request.Method]
			if !ok || (rpcFunc.WebSocketOnly()) {
				responses = append(responses, types.RPCMethodNotFoundError(request.ID))
				cache = false
				continue
			}
			ctx := httpRequestContext(r, &request)
			args := []reflect.Value{reflect.ValueOf(ctx)}
			if len(request.Params) > 0 {
				fnArgs, err := jsonParamsToArgs(rpcFunc, request.Params)
				if err != nil {
					responses = append(
						responses,
						types.RPCInvalidParamsError(request.ID, fmt.Errorf("error converting json params to arguments: %w", err)),
					)
					cache = false
					continue
				}
				args = append(args, fnArgs...)
			}

			if cache && !rpcFunc.CacheableWithArgs(args) {
				cache = false
			}

			returns := rpcFunc.Call(args)
			result, err := unreflectResult(returns)
			if err != nil {
				responses = append(responses, types.RPCInternalError(request.ID, err))
				continue
			}
			responses = append(responses, types.NewRPCSuccessResponse(request.ID, result))
		}

		if len(responses) > 0 {
			var wErr error
			if cache {
				wErr = WriteCacheableRPCResponseHTTP(w, responses...)
			} else {
				wErr = WriteRPCResponseHTTP(w, responses...)
			}
			if wErr != nil {
				logger.Error("failed to write responses", "err", wErr)
			}
		}
	}
}

func handleInvalidJSONRPCPaths(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// Since the pattern "/" matches all paths not matched by other registered patterns,
		//  we check whether the path is indeed "/", otherwise return a 404 error
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}

		next(w, r)
	}
}

// writes a list of available rpc endpoints as an html page
func writeListOfEndpoints(w http.ResponseWriter, r *http.Request, funcMap map[string]*RPCFunc) {
	noArgNames := []string{}
	argNames := []string{}
	for name, funcData := range funcMap {
		if funcData.ArgCount() == 0 {
			noArgNames = append(noArgNames, name)
		} else {
			argNames = append(argNames, name)
		}
	}
	sort.Strings(noArgNames)
	sort.Strings(argNames)
	// The Host comes straight from the request, so escape it before it lands in
	// the page to keep it from breaking out of the surrounding markup.
	host := html.EscapeString(r.Host)
	buf := new(bytes.Buffer)
	buf.WriteString("<html><body>")
	buf.WriteString("<br>Available endpoints:<br>")

	for _, name := range noArgNames {
		link := fmt.Sprintf("//%s/%s", host, name)
		fmt.Fprintf(buf, "<a href=\"%s\">%s</a></br>", link, link)
	}

	buf.WriteString("<br>Endpoints that require arguments:<br>")
	for _, name := range argNames {
		link := fmt.Sprintf("//%s/%s?", host, name)
		funcData := funcMap[name]
		for i, argName := range funcData.ArgNames() {
			link += argName + "=_"
			if i < len(funcData.ArgNames())-1 {
				link += "&"
			}
		}
		fmt.Fprintf(buf, "<a href=\"%s\">%s</a></br>", link, link)
	}
	buf.WriteString("</body></html>")
	w.Header().Set("Content-Type", "text/html")
	w.WriteHeader(200)
	w.Write(buf.Bytes()) //nolint: errcheck
}
