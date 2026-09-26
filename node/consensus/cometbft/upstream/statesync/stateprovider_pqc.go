//go:build dytallix_pqc_only

package statesync

import (
	"context"
	"errors"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/light"
	cmtstate "github.com/cometbft/cometbft/proto/tendermint/state"
)

func NewLightClientStateProvider(context.Context, string, cmtstate.Version, int64, []string, light.TrustOptions, log.Logger) (StateProvider, error) {
	return nil, errors.New("HTTP state provider excluded by dytallix_pqc_only")
}
