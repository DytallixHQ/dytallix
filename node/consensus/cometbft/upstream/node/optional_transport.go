package node

import "github.com/cometbft/cometbft/p2p"

type optionalSwitchReactor struct {
	Name    string
	Reactor p2p.Reactor
}
