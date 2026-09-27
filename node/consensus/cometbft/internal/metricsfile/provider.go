package metricsfile

import (
	"github.com/go-kit/kit/metrics"

	"github.com/cometbft/cometbft/blocksync"
	cs "github.com/cometbft/cometbft/consensus"
	mempl "github.com/cometbft/cometbft/mempool"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/proxy"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/statesync"
)

// Prefix of every engine metric name.
const Prefix = "dytallix_engine_"

// Provider records the core engine metric set (metrics v1, decision 2) in
// r; every other CometBFT metric stays a no-op. The only labels CometBFT
// adds to these are this node's validator address and the consensus step.
func Provider(r *Registry) node.MetricsProvider {
	return func(string) (*cs.Metrics, *p2p.Metrics, *mempl.Metrics, *sm.Metrics, *proxy.Metrics, *blocksync.Metrics, *statesync.Metrics) {
		consensus := cs.NopMetrics()
		consensus.Height = r.Gauge(Prefix + "consensus_height")
		consensus.CommittedHeight = r.Gauge(Prefix + "consensus_latest_block_height")
		consensus.Rounds = r.Gauge(Prefix + "consensus_rounds")
		consensus.StepDurationSeconds = r.Histogram(Prefix + "consensus_step_duration_seconds")
		consensus.BlockIntervalSeconds = r.Histogram(Prefix + "consensus_block_interval_seconds")
		consensus.Validators = r.Gauge(Prefix + "consensus_validators")
		consensus.ValidatorsPower = r.Gauge(Prefix + "consensus_validators_power")
		consensus.MissingValidators = r.Gauge(Prefix + "consensus_missing_validators")
		consensus.MissingValidatorsPower = r.Gauge(Prefix + "consensus_missing_validators_power")
		consensus.ByzantineValidators = r.Gauge(Prefix + "consensus_byzantine_validators")
		consensus.ValidatorMissedBlocks = r.Gauge(Prefix + "consensus_validator_missed_blocks")
		consensus.ValidatorLastSignedHeight = r.Gauge(Prefix + "consensus_validator_last_signed_height")
		peers := p2p.NopMetrics()
		peers.Peers = r.Gauge(Prefix + "p2p_peers")
		mempool := mempl.NopMetrics()
		mempool.Size = r.Gauge(Prefix + "mempool_size")
		mempool.SizeBytes = r.Gauge(Prefix + "mempool_size_bytes")
		mempool.FailedTxs = r.Counter(Prefix + "mempool_failed_txs")
		mempool.RejectedTxs = r.Counter(Prefix + "mempool_rejected_txs")
		blocks := blocksync.NopMetrics()
		blocks.Syncing = r.Gauge(Prefix + "blocksync_syncing")
		blocks.LatestBlockHeight = r.Gauge(Prefix + "blocksync_latest_block_height")
		snapshots := statesync.NopMetrics()
		snapshots.Syncing = r.Gauge(Prefix + "statesync_syncing")
		// Unlabeled gauges and counters start at zero, so the core set is
		// present from the first write; summaries appear with a first value.
		for _, g := range []metrics.Gauge{consensus.Height, consensus.CommittedHeight, consensus.Rounds,
			consensus.Validators, consensus.ValidatorsPower, consensus.MissingValidators,
			consensus.MissingValidatorsPower, consensus.ByzantineValidators, peers.Peers, mempool.Size,
			mempool.SizeBytes, blocks.Syncing, blocks.LatestBlockHeight, snapshots.Syncing} {
			g.Set(0)
		}
		mempool.FailedTxs.Add(0)
		mempool.RejectedTxs.Add(0)
		return consensus, peers, mempool, sm.NopMetrics(), proxy.NopMetrics(), blocks, snapshots
	}
}
