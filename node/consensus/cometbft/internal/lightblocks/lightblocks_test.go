package lightblocks

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/tmhash"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/light"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	cmtversion "github.com/cometbft/cometbft/proto/tendermint/version"
	"github.com/cometbft/cometbft/types"
	"github.com/cometbft/cometbft/version"
)

const testChain = "lightblocks-test"

// chain is a sequence of ML-DSA-65 signed light blocks with the parameters
// in effect at each height. Its validator set grows at changeAt.
type chain struct {
	blocks  map[int64]*types.LightBlock
	params  types.ConsensusParams
	signers []types.PrivValidator
}

func testParams() types.ConsensusParams {
	params := *types.DefaultConsensusParams()
	params.Validator.PubKeyTypes = []string{mldsa65.KeyType}
	return params
}

func signer(t *testing.T) types.PrivValidator {
	t.Helper()
	key, err := mldsa65.GenPrivKey()
	if err != nil {
		t.Fatal(err)
	}
	return types.NewMockPVWithParams(key, false, false)
}

func validatorSet(t *testing.T, signers []types.PrivValidator) *types.ValidatorSet {
	t.Helper()
	var validators []*types.Validator
	for _, pv := range signers {
		pub, err := pv.GetPubKey()
		if err != nil {
			t.Fatal(err)
		}
		validators = append(validators, types.NewValidator(pub, 10))
	}
	return types.NewValidatorSet(validators)
}

// newChain signs heights 1..n with appHash(h) as each header's app hash.
func newChain(t *testing.T, n, changeAt int64, appHash func(int64) []byte) *chain {
	t.Helper()
	first := []types.PrivValidator{signer(t), signer(t)}
	later := append(append([]types.PrivValidator{}, first...), signer(t))
	setAt := func(h int64) []types.PrivValidator {
		if h >= changeAt {
			return later
		}
		return first
	}
	c := &chain{blocks: map[int64]*types.LightBlock{}, params: testParams(), signers: later}
	start := time.Now().Add(-time.Hour)
	var last types.BlockID
	for h := int64(1); h <= n; h++ {
		signers, vals, next := setAt(h), validatorSet(t, setAt(h)), validatorSet(t, setAt(h+1))
		header := &types.Header{
			Version:            cmtversion.Consensus{Block: version.BlockProtocol, App: 1},
			ChainID:            testChain,
			Height:             h,
			Time:               start.Add(time.Duration(h) * time.Second),
			LastBlockID:        last,
			LastCommitHash:     tmhash.Sum([]byte("last commit")),
			DataHash:           tmhash.Sum([]byte("data")),
			ValidatorsHash:     vals.Hash(),
			NextValidatorsHash: next.Hash(),
			ConsensusHash:      c.params.Hash(),
			AppHash:            appHash(h),
			LastResultsHash:    tmhash.Sum([]byte("results")),
			EvidenceHash:       tmhash.Sum([]byte("evidence")),
			ProposerAddress:    vals.Proposer.Address,
		}
		id := types.BlockID{Hash: header.Hash(), PartSetHeader: types.PartSetHeader{Total: 1, Hash: tmhash.Sum([]byte("parts"))}}
		// Votes follow the validator set's order.
		ordered := make([]types.PrivValidator, len(vals.Validators))
		for _, pv := range signers {
			pub, _ := pv.GetPubKey()
			index, _ := vals.GetByAddress(pub.Address())
			ordered[index] = pv
		}
		voteSet := types.NewVoteSet(testChain, h, 0, cmtproto.PrecommitType, vals)
		extended, err := types.MakeExtCommit(id, h, 0, voteSet, ordered, header.Time.Add(time.Second), false)
		if err != nil {
			t.Fatal(err)
		}
		c.blocks[h] = &types.LightBlock{
			SignedHeader: &types.SignedHeader{Header: header, Commit: extended.ToCommit()},
			ValidatorSet: vals,
		}
		last = id
	}
	return c
}

// export writes heights from..to of c into a new directory.
func (c *chain) export(t *testing.T, from, to int64) string {
	t.Helper()
	dir := t.TempDir()
	for h := from; h <= to; h++ {
		if err := Write(dir, testChain, c.blocks[h], &c.params); err != nil {
			t.Fatal(err)
		}
	}
	return dir
}

func (c *chain) trust(height int64) light.TrustOptions {
	return light.TrustOptions{Period: 24 * time.Hour, Height: height, Hash: c.blocks[height].Hash()}
}

func stateProvider(t *testing.T, c *chain, trustAt int64, dirs ...string) (*StateProvider, error) {
	t.Helper()
	return NewStateProvider(context.Background(), testChain, 1, dirs, c.trust(trustAt), log.NewNopLogger())
}

func appHash(h int64) []byte { return tmhash.Sum([]byte{byte(h)}) }

// A snapshot at 7 verified from a trusted height 3 across a validator set
// change at 6: the application hash comes from header 8, the state takes
// its sets from 7, 8 and 9 and the parameters at 8.
func TestStateProviderVerifiesAcrossASetChange(t *testing.T) {
	c := newChain(t, 10, 6, appHash)
	sp, err := stateProvider(t, c, 3, c.export(t, 3, 10))
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	got, err := sp.AppHash(ctx, 7)
	if err != nil || string(got) != string(appHash(8)) {
		t.Fatal(got, err)
	}
	commit, err := sp.Commit(ctx, 7)
	if err != nil || commit.BlockID.Hash.String() != c.blocks[7].Hash().String() {
		t.Fatal(commit, err)
	}
	state, err := sp.State(ctx, 7)
	if err != nil {
		t.Fatal(err)
	}
	if state.LastBlockHeight != 7 || string(state.AppHash) != string(appHash(8)) ||
		state.LastValidators.Size() != 3 || state.Validators.Size() != 3 || state.NextValidators.Size() != 3 ||
		state.InitialHeight != 1 || state.ChainID != testChain || string(state.ConsensusParams.Hash()) != string(c.params.Hash()) {
		t.Fatalf("%+v", state)
	}
	earlier, err := sp.State(ctx, 4)
	if err != nil || earlier.LastValidators.Size() != 2 || earlier.NextValidators.Size() != 3 {
		t.Fatal(earlier, err)
	}
}

// Every header is verified: a gap stops sequential verification, a header
// changed after signing and a wrong trusted hash are refused, and exported
// parameters must match the header.
func TestStateProviderRefusesGapsAlterationsAndForeignParameters(t *testing.T) {
	c := newChain(t, 10, 6, appHash)
	ctx := context.Background()
	gap := c.export(t, 3, 10)
	if err := os.Remove(filepath.Join(gap, fileName(5, "block"))); err != nil {
		t.Fatal(err)
	}
	sp, err := stateProvider(t, c, 3, gap)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.AppHash(ctx, 7); err == nil {
		t.Fatal("verified across a missing header")
	}
	// Header 8 with another application hash, under the original commit.
	altered := c.export(t, 3, 10)
	block := *c.blocks[8]
	header := *block.Header
	header.AppHash = appHash(99)
	block.SignedHeader = &types.SignedHeader{Header: &header, Commit: block.Commit}
	pb, err := block.ToProto()
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := pb.Marshal()
	if err = os.WriteFile(filepath.Join(altered, fileName(8, "block")), raw, 0o600); err != nil {
		t.Fatal(err)
	}
	sp, err = stateProvider(t, c, 3, altered)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.AppHash(ctx, 7); err == nil {
		t.Fatal("verified an altered header")
	}
	if _, err = NewStateProvider(ctx, testChain, 1, []string{c.export(t, 3, 10)},
		light.TrustOptions{Period: 24 * time.Hour, Height: 3, Hash: c.blocks[4].Hash()}, log.NewNopLogger()); err == nil {
		t.Fatal("accepted a wrong trusted hash")
	}
	foreign := c.export(t, 3, 10)
	params := c.params
	params.Block.MaxBytes++
	if err = Write(foreign, testChain, c.blocks[8], &params); err != nil {
		t.Fatal(err)
	}
	sp, err = stateProvider(t, c, 3, foreign)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.State(ctx, 7); err == nil || !strings.Contains(err.Error(), "differ from the verified header") {
		t.Fatal(err)
	}
}

// A witness export holding a conflicting, validly signed header stops
// verification.
func TestStateProviderStopsOnAConflictingWitness(t *testing.T) {
	c := newChain(t, 10, 6, appHash)
	fork := *c
	fork.blocks = map[int64]*types.LightBlock{}
	for h, b := range c.blocks {
		fork.blocks[h] = b
	}
	// The same validators sign another header at 8.
	other := newChainWithSigners(t, c, 8, appHash(77))
	fork.blocks[8] = other
	sp, err := stateProvider(t, c, 3, c.export(t, 3, 10), fork.export(t, 3, 10))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.AppHash(context.Background(), 7); err == nil {
		t.Fatal("verified a header a witness contradicts")
	}
}

// newChainWithSigners re-signs c's header at height with another app hash.
func newChainWithSigners(t *testing.T, c *chain, height int64, app []byte) *types.LightBlock {
	t.Helper()
	original := c.blocks[height]
	header := *original.Header
	header.AppHash = app
	id := types.BlockID{Hash: header.Hash(), PartSetHeader: original.Commit.BlockID.PartSetHeader}
	vals := original.ValidatorSet
	ordered := make([]types.PrivValidator, len(vals.Validators))
	for _, pv := range c.signers {
		pub, _ := pv.GetPubKey()
		if index, v := vals.GetByAddress(pub.Address()); v != nil {
			ordered[index] = pv
		}
	}
	voteSet := types.NewVoteSet(testChain, height, 0, cmtproto.PrecommitType, vals)
	extended, err := types.MakeExtCommit(id, height, 0, voteSet, ordered, header.Time.Add(time.Second), false)
	if err != nil {
		t.Fatal(err)
	}
	return &types.LightBlock{SignedHeader: &types.SignedHeader{Header: &header, Commit: extended.ToCommit()}, ValidatorSet: vals}
}

// Export reads each height's header, commit, validators and parameters, and
// takes the seen commit for the last height.
type fakeStores struct{ c *chain }

func (f fakeStores) LoadBlockMeta(h int64) *types.BlockMeta {
	b, ok := f.c.blocks[h]
	if !ok {
		return nil
	}
	return &types.BlockMeta{BlockID: b.Commit.BlockID, Header: *b.Header}
}
func (f fakeStores) LoadBlockCommit(h int64) *types.Commit {
	if _, ok := f.c.blocks[h+1]; !ok {
		return nil
	}
	return f.c.blocks[h].Commit
}
func (f fakeStores) LoadSeenCommit(h int64) *types.Commit {
	if b, ok := f.c.blocks[h]; ok {
		return b.Commit
	}
	return nil
}
func (f fakeStores) LoadValidators(h int64) (*types.ValidatorSet, error) {
	return f.c.blocks[h].ValidatorSet, nil
}
func (f fakeStores) LoadConsensusParams(int64) (types.ConsensusParams, error) {
	return f.c.params, nil
}

func TestExportedLightBlocksServeTheStateProvider(t *testing.T) {
	c := newChain(t, 10, 6, appHash)
	dir := t.TempDir()
	if err := Export(fakeStores{c}, fakeStores{c}, testChain, 3, 10, dir); err != nil {
		t.Fatal(err)
	}
	sp, err := stateProvider(t, c, 3, dir)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = sp.State(context.Background(), 7); err != nil {
		t.Fatal(err)
	}
	p, _ := NewProvider(testChain, dir)
	latest, err := p.LightBlock(context.Background(), 0)
	if err != nil || latest.Height != 10 {
		t.Fatal(latest, err)
	}
	if err = Export(fakeStores{c}, fakeStores{c}, testChain, 3, 11, t.TempDir()); err == nil {
		t.Fatal("exported a height the node does not hold")
	}
	for _, bad := range [][2]int64{{0, 3}, {5, 4}, {1, MaxExport + 1}} {
		if err = Export(fakeStores{c}, fakeStores{c}, testChain, bad[0], bad[1], t.TempDir()); err == nil {
			t.Fatal("accepted export range", bad)
		}
	}
}
