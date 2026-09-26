//go:build dytallix_pqc_only

package types

import "context"

// Context carries the same request and subscription data without importing an HTTP implementation.
type Context struct {
	JSONReq        *RPCRequest
	WSConn         WSRPCConnection
	requestContext context.Context
	requestRemote  string
}

func NewRequestContext(ctx context.Context, remote string, request *RPCRequest) *Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return &Context{JSONReq: request, requestContext: ctx, requestRemote: remote}
}
func (ctx *Context) RemoteAddr() string {
	if ctx.requestContext != nil {
		return ctx.requestRemote
	}
	if ctx.WSConn != nil {
		return ctx.WSConn.GetRemoteAddr()
	}
	return ""
}
func (ctx *Context) Context() context.Context {
	if ctx.requestContext != nil {
		return ctx.requestContext
	}
	if ctx.WSConn != nil {
		return ctx.WSConn.Context()
	}
	return context.Background()
}
