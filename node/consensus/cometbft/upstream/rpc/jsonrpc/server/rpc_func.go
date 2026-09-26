package server

import (
	"fmt"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/rpc/jsonrpc/dispatch"
	"net/http"
	"reflect"
)

// RegisterRPCFuncs adds a route for each function in the funcMap, as well as
// general jsonrpc and websocket handlers for all functions. "result" is the
// interface on which the result objects are registered, and is populated with
// every RPCResponse
func RegisterRPCFuncs(mux *http.ServeMux, funcMap map[string]*RPCFunc, logger log.Logger) {
	// HTTP endpoints
	for funcName, rpcFunc := range funcMap {
		mux.HandleFunc("/"+funcName, makeHTTPHandler(rpcFunc, logger))
	}

	// JSONRPC endpoints
	mux.HandleFunc("/", handleInvalidJSONRPCPaths(makeJSONRPCHandler(funcMap, logger)))
}

// Aliases retain the existing adapter API while the core owns transport-neutral descriptors.
type RPCFunc = dispatch.RPCFunc
type Option = dispatch.Option

func Cacheable(args ...string) Option { return dispatch.Cacheable(args...) }
func Ws() Option                      { return dispatch.Ws() }
func NewRPCFunc(f any, args string, options ...Option) *RPCFunc {
	return dispatch.NewRPCFunc(f, args, options...)
}
func NewWSRPCFunc(f any, args string, options ...Option) *RPCFunc {
	return dispatch.NewWSRPCFunc(f, args, options...)
}

//-------------------------------------------------------------

// NOTE: assume returns is result struct and error. If error is not nil, return it
func unreflectResult(returns []reflect.Value) (any, error) {
	errV := returns[1]
	if errV.Interface() != nil {
		return nil, fmt.Errorf("%v", errV.Interface())
	}
	rv := returns[0]
	// the result is a registered interface,
	// we need a pointer to it so we can marshal with type byte
	rvp := reflect.New(rv.Type())
	rvp.Elem().Set(rv)
	return rvp.Interface(), nil
}
