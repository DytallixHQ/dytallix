//go:build !dytallix_pqc_only

package node

import (
	"fmt"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/lp2p"
	"github.com/cometbft/cometbft/p2p"
)

func createOptionalSwitch(config *cfg.Config, nodeInfo p2p.NodeInfo, nodeKey *p2p.NodeKey, reactors []optionalSwitchReactor, metrics *p2p.Metrics, logger log.Logger, height int64) (p2p.Switcher, error) {

	mapped := make([]lp2p.SwitchReactor, len(reactors))
	for i, r := range reactors {
		mapped[i] = lp2p.SwitchReactor{Name: r.Name, Reactor: r.Reactor}
	}
	host, err := lp2p.NewHost(config.P2P, nodeKey.PrivKey, logger)
	if err != nil {
		return nil, fmt.Errorf("unable to create libp2p host: %w", err)
	}
	sw, err := lp2p.NewSwitch(nodeInfo, host, mapped, metrics, logger)
	if err != nil {
		return nil, fmt.Errorf("unable to create libp2p switch: %w", err)
	}
	logger.Info("Using libp2p transport", "host_id", host.ID().String())
	if height != 0 {
		logger.Warn("EXPERIMENTAL: go-libp2p transport is enabled. Only enable this setting if it can be activated simultaneously for all validators on the network and peer IDs have been predetermined and exchanged.")
	}
	return sw, nil
}
