//go:build dytallix_pqc_only

package block

import (
	"errors"
	"github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/state/indexer"
	"github.com/cometbft/cometbft/state/txindex"
)

func sqlIndexerForBuild(cfg *config.Config, chainID string) (txindex.TxIndexer, indexer.BlockIndexer, bool, error) {
	return nil, nil, false, errors.New("SQL indexer excluded by dytallix_pqc_only")
}
