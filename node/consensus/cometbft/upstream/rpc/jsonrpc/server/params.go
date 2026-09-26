package server

import (
	"github.com/cometbft/cometbft/rpc/jsonrpc/dispatch"
	"net/http"
	"reflect"
)

func jsonParamsToArgs(f *RPCFunc, raw []byte) ([]reflect.Value, error) {
	return dispatch.JSONParamsToArgs(f, raw)
}
func httpParamsToArgs(f *RPCFunc, r *http.Request) ([]reflect.Value, error) {
	return dispatch.URIParamsToArgs(f, func(name string) string { return getParam(r, name) })
}
