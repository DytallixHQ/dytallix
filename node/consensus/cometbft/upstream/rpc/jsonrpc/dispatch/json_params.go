package dispatch

import (
	"encoding/json"
	"fmt"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"reflect"
)

func mapParamsToArgs(
	rpcFunc *RPCFunc,
	params map[string]json.RawMessage,
	argsOffset int,
) ([]reflect.Value, error) {
	values := make([]reflect.Value, len(rpcFunc.ArgNames()))
	for i, argName := range rpcFunc.ArgNames() {
		argType := rpcFunc.ArgType(i + argsOffset)

		if p, ok := params[argName]; ok && p != nil && len(p) > 0 {
			val := reflect.New(argType)
			err := cmtjson.Unmarshal(p, val.Interface())
			if err != nil {
				return nil, err
			}
			values[i] = val.Elem()
		} else { // use default for that type
			values[i] = reflect.Zero(argType)
		}
	}

	return values, nil
}

func arrayParamsToArgs(
	rpcFunc *RPCFunc,
	params []json.RawMessage,
	argsOffset int,
) ([]reflect.Value, error) {
	if len(rpcFunc.ArgNames()) != len(params) {
		return nil, fmt.Errorf("expected %v parameters (%v), got %v (%v)",
			len(rpcFunc.ArgNames()), rpcFunc.ArgNames(), len(params), params)
	}

	values := make([]reflect.Value, len(params))
	for i, p := range params {
		argType := rpcFunc.ArgType(i + argsOffset)
		val := reflect.New(argType)
		err := cmtjson.Unmarshal(p, val.Interface())
		if err != nil {
			return nil, err
		}
		values[i] = val.Elem()
	}
	return values, nil
}

// raw is unparsed json (from json.RawMessage) encoding either a map or an
// array.
//
// Example:
//
//	rpcFunc.args = [rpctypes.Context string]
//	rpcFunc.ArgNames() = ["arg"]
func JSONParamsToArgs(rpcFunc *RPCFunc, raw []byte) ([]reflect.Value, error) {
	const argsOffset = 1

	// TODO: Make more efficient, perhaps by checking the first character for '{' or '['?
	// First, try to get the map.
	var m map[string]json.RawMessage
	err := json.Unmarshal(raw, &m)
	if err == nil {
		return mapParamsToArgs(rpcFunc, m, argsOffset)
	}

	// Otherwise, try an array.
	var a []json.RawMessage
	err = json.Unmarshal(raw, &a)
	if err == nil {
		return arrayParamsToArgs(rpcFunc, a, argsOffset)
	}

	// Otherwise, bad format, we cannot parse
	return nil, fmt.Errorf("unknown type for JSON params: %v. Expected map or array", err)
}
