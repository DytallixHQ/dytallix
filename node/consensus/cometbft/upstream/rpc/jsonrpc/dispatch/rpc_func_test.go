package dispatch

import (
	types "github.com/cometbft/cometbft/rpc/jsonrpc/types"
	"reflect"
	"testing"
)

func TestDescriptorPreservesCallsCacheAndMetadata(t *testing.T) {
	f := NewRPCFunc(func(_ *types.Context, height int) (int, error) { return height, nil }, "height", Cacheable("height"))
	args := []reflect.Value{reflect.ValueOf(&types.Context{}), reflect.ValueOf(7)}
	if f.ArgCount() != 2 || f.ArgType(1).Kind() != reflect.Int || f.WebSocketOnly() {
		t.Fatal("descriptor changed")
	}
	if !f.CacheableWithArgs(args) || f.CacheableWithArgs([]reflect.Value{args[0], reflect.ValueOf(0)}) || f.CacheableWithArgs(args[:1]) {
		t.Fatal("cache decision changed")
	}
	result := f.Call(args)
	if result[0].Int() != 7 || !result[1].IsNil() {
		t.Fatal("method result changed")
	}
	names := f.ArgNames()
	names[0] = "edited"
	if f.ArgNames()[0] != "height" {
		t.Fatal("caller mutated descriptor metadata")
	}
	if !NewWSRPCFunc(func(*types.Context) {}, "").WebSocketOnly() {
		t.Fatal("WebSocket method changed")
	}
}
