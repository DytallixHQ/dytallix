//go:build !dytallix_pqc_only

package enginepqc

import (
	"bytes"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/spf13/viper"
)

func decodeConfig(raw []byte) (*cfg.Config, error) {
	v := viper.New()
	v.SetConfigType("toml")
	if err := v.ReadConfig(bytes.NewReader(raw)); err != nil {
		return nil, err
	}
	c := cfg.DefaultConfig()
	if err := v.UnmarshalExact(c); err != nil {
		return nil, err
	}
	return c, nil
}
