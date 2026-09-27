package lightblocks

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"sync"
	"time"

	dbm "github.com/cometbft/cometbft-db"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/light"
	"github.com/cometbft/cometbft/light/provider"
	lightdb "github.com/cometbft/cometbft/light/store/db"
	cmtstate "github.com/cometbft/cometbft/proto/tendermint/state"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/statesync"
	"github.com/cometbft/cometbft/types"
	"github.com/cometbft/cometbft/version"
)

// StateProvider gives state sync the application hash, commit and state at a
// snapshot height from exported light blocks, each verified by the light
// client. It replaces CometBFT's RPC state provider.
type StateProvider struct {
	mu            sync.Mutex
	lc            *light.Client
	primary       *Provider
	initialHeight int64
}

var _ statesync.StateProvider = (*StateProvider)(nil)

// NewStateProvider verifies from the operator's trusted height, hash and
// period by sequential verification, so every height from the trusted one
// through the snapshot height plus two must be exported. The first
// directory is the primary; the others, exports from other nodes the
// operator runs, are witnesses. A single export is its own witness, which
// adds no cross-check.
func NewStateProvider(
	ctx context.Context,
	chainID string,
	initialHeight int64,
	dirs []string,
	trust light.TrustOptions,
	logger log.Logger,
) (*StateProvider, error) {
	if len(dirs) == 0 {
		return nil, errors.New("state sync requires at least one light block export")
	}
	var providers []provider.Provider
	var primary *Provider
	for i, dir := range dirs {
		p, err := NewProvider(chainID, dir)
		if err != nil {
			return nil, fmt.Errorf("light block export %d: %w", i, err)
		}
		if i == 0 {
			primary = p
		}
		providers = append(providers, p)
	}
	witnesses := providers[1:]
	if len(witnesses) == 0 {
		witnesses = providers
	}
	lc, err := light.NewClient(ctx, chainID, trust, primary, witnesses,
		lightdb.New(dbm.NewMemDB(), ""), light.Logger(logger), light.SequentialVerification(),
		light.MaxRetryAttempts(1))
	if err != nil {
		return nil, err
	}
	if initialHeight == 0 {
		initialHeight = 1
	}
	return &StateProvider{lc: lc, primary: primary, initialHeight: initialHeight}, nil
}

func (s *StateProvider) verified(ctx context.Context, height uint64) (*types.LightBlock, error) {
	return s.lc.VerifyLightBlockAtHeight(ctx, int64(height), time.Now())
}

// AppHash returns the application hash after height, from the header at
// height+1. It also verifies height+2, which State needs, so that a
// snapshot whose light blocks are incomplete is refused when offered.
func (s *StateProvider) AppHash(ctx context.Context, height uint64) ([]byte, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	next, err := s.verified(ctx, height+1)
	if err != nil {
		return nil, err
	}
	if _, err = s.verified(ctx, height+2); err != nil {
		return nil, err
	}
	return next.AppHash, nil
}

// Commit returns the verified commit for height.
func (s *StateProvider) Commit(ctx context.Context, height uint64) (*types.Commit, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	block, err := s.verified(ctx, height)
	if err != nil {
		return nil, err
	}
	return block.Commit, nil
}

// State builds the state after height: height is the last block, height+1
// the first block after the snapshot and height+2 the one after, whose
// validators take effect if the snapshot height changed the set. The
// consensus parameters at height+1 must hash to that header's consensus hash.
func (s *StateProvider) State(ctx context.Context, height uint64) (sm.State, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	last, err := s.verified(ctx, height)
	if err != nil {
		return sm.State{}, err
	}
	current, err := s.verified(ctx, height+1)
	if err != nil {
		return sm.State{}, err
	}
	next, err := s.verified(ctx, height+2)
	if err != nil {
		return sm.State{}, err
	}
	params, err := s.primary.ConsensusParams(current.Height)
	if err != nil {
		return sm.State{}, fmt.Errorf("consensus parameters at %d: %w", current.Height, err)
	}
	if !bytes.Equal(params.Hash(), current.ConsensusHash) {
		return sm.State{}, fmt.Errorf("consensus parameters at %d differ from the verified header", current.Height)
	}
	return sm.State{
		ChainID:       s.lc.ChainID(),
		InitialHeight: s.initialHeight,
		Version: cmtstate.Version{
			Consensus: current.Version,
			Software:  version.TMCoreSemVer,
		},
		LastBlockHeight:                  last.Height,
		LastBlockTime:                    last.Time,
		LastBlockID:                      last.Commit.BlockID,
		AppHash:                          current.AppHash,
		LastResultsHash:                  current.LastResultsHash,
		LastValidators:                   last.ValidatorSet,
		Validators:                       current.ValidatorSet,
		NextValidators:                   next.ValidatorSet,
		LastHeightValidatorsChanged:      next.Height,
		ConsensusParams:                  params,
		LastHeightConsensusParamsChanged: current.Height,
	}, nil
}
