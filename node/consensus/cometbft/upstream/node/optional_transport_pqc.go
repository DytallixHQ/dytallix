//go:build dytallix_pqc_only

package node

import (
	"fmt"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
)

func createOptionalSwitch(config *cfg.Config, nodeInfo p2p.NodeInfo, nodeKey *p2p.NodeKey, reactors []optionalSwitchReactor, metrics *p2p.Metrics, logger log.Logger, height int64) (p2p.Switcher, error) {
	return nil, fmt.Errorf("libp2p excluded by dytallix_pqc_only")
}
