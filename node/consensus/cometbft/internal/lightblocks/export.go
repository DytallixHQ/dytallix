package lightblocks

import (
	"errors"
	"fmt"

	"github.com/cometbft/cometbft/types"
)

// MaxExport bounds the heights one export holds.
const MaxExport = 100_000

// BlockReader is the part of a node's block store an export reads.
type BlockReader interface {
	LoadBlockMeta(height int64) *types.BlockMeta
	LoadBlockCommit(height int64) *types.Commit
	LoadSeenCommit(height int64) *types.Commit
}

// StateReader is the part of a node's state store an export reads.
type StateReader interface {
	LoadValidators(height int64) (*types.ValidatorSet, error)
	LoadConsensusParams(height int64) (types.ConsensusParams, error)
}

// Export writes the light block and consensus parameters of every height from
// through to, read from a node's stores, into dir. The node must hold the
// commit for to: its next block, or the commit it saw.
func Export(blocks BlockReader, states StateReader, chainID string, from, to int64, dir string) error {
	if from < 1 || to < from || to-from >= MaxExport {
		return errors.New("export heights out of range")
	}
	for height := from; height <= to; height++ {
		meta := blocks.LoadBlockMeta(height)
		if meta == nil {
			return fmt.Errorf("block %d is not held", height)
		}
		commit := blocks.LoadBlockCommit(height)
		if commit == nil {
			commit = blocks.LoadSeenCommit(height)
		}
		if commit == nil {
			return fmt.Errorf("commit for block %d is not held", height)
		}
		validators, err := states.LoadValidators(height)
		if err != nil {
			return fmt.Errorf("validators at %d: %w", height, err)
		}
		params, err := states.LoadConsensusParams(height)
		if err != nil {
			return fmt.Errorf("consensus parameters at %d: %w", height, err)
		}
		block := &types.LightBlock{
			SignedHeader: &types.SignedHeader{Header: &meta.Header, Commit: commit},
			ValidatorSet: validators,
		}
		if err = Write(dir, chainID, block, &params); err != nil {
			return fmt.Errorf("light block %d: %w", height, err)
		}
	}
	return nil
}
