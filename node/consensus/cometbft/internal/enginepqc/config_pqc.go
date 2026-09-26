//go:build dytallix_pqc_only

package enginepqc

import (
	"fmt"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/go-viper/mapstructure/v2"
	"github.com/pelletier/go-toml/v2"
	"reflect"
	"strings"
)

// Configuration remains case-insensitive. Reject collisions instead of depending on map iteration order.
func canonicalConfigKeys(in map[string]any) (map[string]any, error) {
	out := make(map[string]any, len(in))
	for key, value := range in {
		lower := strings.ToLower(key)
		if _, exists := out[lower]; exists {
			return nil, fmt.Errorf("configuration keys collide ignoring case: %s", lower)
		}
		if nested, ok := value.(map[string]any); ok {
			normalized, err := canonicalConfigKeys(nested)
			if err != nil {
				return nil, err
			}
			value = normalized
		}
		out[lower] = value
	}
	return out, nil
}

// Match the legacy decoder's duration and weak comma-separated slice conversion.
func configSliceHook(from, to reflect.Type, value any) (any, error) {
	if from.Kind() != reflect.String || to.Kind() != reflect.Slice {
		return value, nil
	}
	raw := value.(string)
	if raw == "" {
		return []string{}, nil
	}
	return strings.Split(raw, ","), nil
}
func decodeConfig(raw []byte) (*cfg.Config, error) {
	var values map[string]any
	if err := toml.Unmarshal(raw, &values); err != nil {
		return nil, err
	}
	values, err := canonicalConfigKeys(values)
	if err != nil {
		return nil, err
	}
	c := cfg.DefaultConfig()
	decoder, err := mapstructure.NewDecoder(&mapstructure.DecoderConfig{
		Result: c, ErrorUnused: true, WeaklyTypedInput: true,
		DecodeHook: mapstructure.ComposeDecodeHookFunc(mapstructure.StringToTimeDurationHookFunc(), configSliceHook),
	})
	if err != nil {
		return nil, err
	}
	if err = decoder.Decode(values); err != nil {
		return nil, err
	}
	return c, nil
}
