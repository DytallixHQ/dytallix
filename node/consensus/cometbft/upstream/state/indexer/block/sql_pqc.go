package block

import (
	"errors"
	"github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/state/indexer"
	"github.com/cometbft/cometbft/state/txindex"
)

func sqlIndexerForBuild(cfg *config.Config, chainID string) (txindex.TxIndexer, indexer.BlockIndexer, bool, error) {
	return nil, nil, false, errors.New("SQL indexer is not built (Dytallix PQC-only fork)")
}
