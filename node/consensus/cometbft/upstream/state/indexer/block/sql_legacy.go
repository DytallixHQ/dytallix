//go:build !dytallix_pqc_only

package block

import (
	"errors"
	"fmt"
	"github.com/cometbft/cometbft/config"
	"github.com/cometbft/cometbft/state/indexer"
	"github.com/cometbft/cometbft/state/indexer/sink/psql"
	"github.com/cometbft/cometbft/state/txindex"
)

func sqlIndexerForBuild(cfg *config.Config, chainID string) (txindex.TxIndexer, indexer.BlockIndexer, bool, error) {
	conn := cfg.TxIndex.PsqlConn
	if conn == "" {
		return nil, nil, false, errors.New("the psql connection settings cannot be empty")
	}
	es, err := psql.NewEventSink(cfg.TxIndex.PsqlConn, chainID)
	if err != nil {
		return nil, nil, false, fmt.Errorf("creating psql indexer: %w", err)
	}
	return es.TxIndexer(), es.BlockIndexer(), false, nil

}
