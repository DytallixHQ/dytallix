//go:build !dytallix_pqc_only

package types

import (
	"context"
	"net/http"
)

// Context is the first parameter for all functions. It carries a json-rpc
// request, http request and websocket connection.
//
// - JSONReq is non-nil when JSONRPC is called over websocket or HTTP.
// - WSConn is non-nil when we're connected via a websocket.
// - HTTPReq is non-nil when URI or JSONRPC is called over HTTP.
type Context struct {
	// json-rpc request
	JSONReq *RPCRequest
	// websocket connection
	WSConn WSRPCConnection
	// http request
	HTTPReq *http.Request
}

// RemoteAddr returns the remote address (usually a string "IP:port").
// If neither HTTPReq nor WSConn is set, an empty string is returned.
// HTTP:
//
//	http.Request#RemoteAddr
//
// WS:
//
//	result of GetRemoteAddr
func (ctx *Context) RemoteAddr() string {
	if ctx.HTTPReq != nil {
		return ctx.HTTPReq.RemoteAddr
	} else if ctx.WSConn != nil {
		return ctx.WSConn.GetRemoteAddr()
	}
	return ""
}

// Context returns the request's context.
// The returned context is always non-nil; it defaults to the background context.
// HTTP:
//
//	The context is canceled when the client's connection closes, the request
//	is canceled (with HTTP/2), or when the ServeHTTP method returns.
//
// WS:
//
//	The context is canceled when the client's connections closes.
func (ctx *Context) Context() context.Context {
	if ctx.HTTPReq != nil {
		return ctx.HTTPReq.Context()
	} else if ctx.WSConn != nil {
		return ctx.WSConn.Context()
	}
	return context.Background()
}

func NewRequestContext(ctx context.Context, remote string, request *RPCRequest) *Context {
	if ctx == nil {
		ctx = context.Background()
	}
	httpRequest := (&http.Request{RemoteAddr: remote}).WithContext(ctx)
	return &Context{JSONReq: request, HTTPReq: httpRequest}
}
