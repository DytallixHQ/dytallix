package server

import (
	"fmt"
	"net/http"
	"reflect"

	"github.com/cometbft/cometbft/libs/log"
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
)

// HTTP + URI handler

// convert from a function name to the http handler
func makeHTTPHandler(rpcFunc *RPCFunc, logger log.Logger) func(http.ResponseWriter, *http.Request) {
	// Always return -1 as there's no ID here.
	dummyID := types.JSONRPCIntID(-1) // URIClientRequestID

	// Exception for websocket endpoints
	if rpcFunc.WebSocketOnly() {
		return func(w http.ResponseWriter, r *http.Request) {
			res := types.RPCMethodNotFoundError(dummyID)
			if wErr := WriteRPCResponseHTTPError(w, http.StatusNotFound, res); wErr != nil {
				logger.Error("failed to write response", "err", wErr)
			}
		}
	}

	// All other endpoints
	return func(w http.ResponseWriter, r *http.Request) {
		logger.Debug("HTTP HANDLER", "req", r)

		ctx := httpRequestContext(r, nil)
		args := []reflect.Value{reflect.ValueOf(ctx)}

		fnArgs, err := httpParamsToArgs(rpcFunc, r)
		if err != nil {
			res := types.RPCInvalidParamsError(dummyID,
				fmt.Errorf("error converting http params to arguments: %w", err),
			)
			if wErr := WriteRPCResponseHTTPError(w, http.StatusInternalServerError, res); wErr != nil {
				logger.Error("failed to write response", "err", wErr)
			}
			return
		}
		args = append(args, fnArgs...)

		returns := rpcFunc.Call(args)

		logger.Debug("HTTPRestRPC", "method", r.URL.Path, "args", args, "returns", returns)
		result, err := unreflectResult(returns)
		if err != nil {
			if err := WriteRPCResponseHTTPError(w, http.StatusInternalServerError,
				types.RPCInternalError(dummyID, err)); err != nil {
				logger.Error("failed to write response", "err", err)
				return
			}
			return
		}

		resp := types.NewRPCSuccessResponse(dummyID, result)
		if rpcFunc.CacheableWithArgs(args) {
			err = WriteCacheableRPCResponseHTTP(w, resp)
		} else {
			err = WriteRPCResponseHTTP(w, resp)
		}
		if err != nil {
			logger.Error("failed to write response", "err", err)
			return
		}
	}
}

func getParam(r *http.Request, param string) string {
	s := r.URL.Query().Get(param)
	if s == "" {
		s = r.FormValue(param)
	}
	return s
}
