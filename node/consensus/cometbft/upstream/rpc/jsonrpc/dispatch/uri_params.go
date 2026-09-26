package dispatch

import (
	"encoding/hex"
	"fmt"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"reflect"
	"regexp"
	"strings"
)

var reInt = regexp.MustCompile(`^-?[0-9]+$`)

// Convert an http query to a list of properly typed values.
// To be properly decoded the arg must be a concrete type from CometBFT (if its an interface).
func URIParamsToArgs(rpcFunc *RPCFunc, lookup func(string) string) ([]reflect.Value, error) {
	// skip types.Context
	const argsOffset = 1

	values := make([]reflect.Value, len(rpcFunc.ArgNames()))

	for i, name := range rpcFunc.ArgNames() {
		argType := rpcFunc.ArgType(i + argsOffset)

		values[i] = reflect.Zero(argType) // set default for that type

		arg := lookup(name)
		// log.Notice("param to arg", "argType", argType, "name", name, "arg", arg)

		if arg == "" {
			continue
		}

		v, ok, err := nonJSONStringToArg(argType, arg)
		if err != nil {
			return nil, err
		}
		if ok {
			values[i] = v
			continue
		}

		values[i], err = jsonStringToArg(argType, arg)
		if err != nil {
			return nil, err
		}
	}

	return values, nil
}

func jsonStringToArg(rt reflect.Type, arg string) (reflect.Value, error) {
	rv := reflect.New(rt)
	err := cmtjson.Unmarshal([]byte(arg), rv.Interface())
	if err != nil {
		return rv, err
	}
	rv = rv.Elem()
	return rv, nil
}

func nonJSONStringToArg(rt reflect.Type, arg string) (reflect.Value, bool, error) {
	if rt.Kind() == reflect.Pointer {
		rv1, ok, err := nonJSONStringToArg(rt.Elem(), arg)
		switch {
		case err != nil:
			return reflect.Value{}, false, err
		case ok:
			rv := reflect.New(rt.Elem())
			rv.Elem().Set(rv1)
			return rv, true, nil
		default:
			return reflect.Value{}, false, nil
		}
	} else {
		return _nonJSONStringToArg(rt, arg)
	}
}

// NOTE: rt.Kind() isn't a pointer.
func _nonJSONStringToArg(rt reflect.Type, arg string) (reflect.Value, bool, error) {
	isIntString := reInt.Match([]byte(arg))
	isQuotedString := strings.HasPrefix(arg, `"`) && strings.HasSuffix(arg, `"`)
	isHexString := strings.HasPrefix(strings.ToLower(arg), "0x")

	var expectingString, expectingByteSlice, expectingInt bool
	switch rt.Kind() {
	case reflect.Int,
		reflect.Uint,
		reflect.Int8,
		reflect.Uint8,
		reflect.Int16,
		reflect.Uint16,
		reflect.Int32,
		reflect.Uint32,
		reflect.Int64,
		reflect.Uint64:
		expectingInt = true
	case reflect.String:
		expectingString = true
	case reflect.Slice:
		expectingByteSlice = rt.Elem().Kind() == reflect.Uint8
	}

	if isIntString && expectingInt {
		qarg := `"` + arg + `"`
		rv, err := jsonStringToArg(rt, qarg)
		if err != nil {
			return rv, false, err
		}

		return rv, true, nil
	}

	if isHexString {
		if !expectingString && !expectingByteSlice {
			err := fmt.Errorf("got a hex string arg, but expected '%s'",
				rt.Kind().String())
			return reflect.ValueOf(nil), false, err
		}

		var value []byte
		value, err := hex.DecodeString(arg[2:])
		if err != nil {
			return reflect.ValueOf(nil), false, err
		}
		if rt.Kind() == reflect.String {
			return reflect.ValueOf(string(value)), true, nil
		}
		return reflect.ValueOf(value), true, nil
	}

	if isQuotedString && expectingByteSlice {
		v := reflect.New(reflect.TypeOf(""))
		err := cmtjson.Unmarshal([]byte(arg), v.Interface())
		if err != nil {
			return reflect.ValueOf(nil), false, err
		}
		v = v.Elem()
		return reflect.ValueOf([]byte(v.String())), true, nil
	}

	return reflect.ValueOf(nil), false, nil
}
