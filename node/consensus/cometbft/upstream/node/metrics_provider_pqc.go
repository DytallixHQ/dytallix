//go:build dytallix_pqc_only

package node

import (
	"github.com/cometbft/cometbft/blocksync"
	cfg "github.com/cometbft/cometbft/config"
	cs "github.com/cometbft/cometbft/consensus"
	mempl "github.com/cometbft/cometbft/mempool"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/proxy"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/statesync"
)

// The selected profile supports only the existing disabled-metrics mode.
// Node construction separately rejects enabled metrics before database work.
func DefaultMetricsProvider(config *cfg.InstrumentationConfig) MetricsProvider {
	return func(string) (*cs.Metrics, *p2p.Metrics, *mempl.Metrics, *sm.Metrics, *proxy.Metrics, *blocksync.Metrics, *statesync.Metrics) {
		if config.Prometheus {
			panic("Prometheus metrics excluded by dytallix_pqc_only")
		}
		return cs.NopMetrics(), p2p.NopMetrics(), mempl.NopMetrics(), sm.NopMetrics(), proxy.NopMetrics(), blocksync.NopMetrics(), statesync.NopMetrics()
	}
}
