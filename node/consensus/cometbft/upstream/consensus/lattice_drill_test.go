package consensus

// Lattice-break drill (crypto-agility spike). ML-DSA-65 is declared untrusted.
//
// Part A, live swap: four ML-DSA-65 validators run the chain. At height 2 one
// block carries validator updates that add four SLH-DSA-SHAKE-192f keys and
// remove every ML-DSA-65 key. CometBFT applies them at height 4, so blocks 2
// and 3 are still ML-DSA-signed. From height 4 every vote, proposal and
// commit is hash-based.
//
// Halt and checkpoint: signing stops after height 6. A root key (SLH-DSA-
// SHAKE-256s, the root authority's algorithm) signs a checkpoint binding block
// 6's hash, the app hash after it and the next validator set.
//
// Part B, restart: a replacement genesis starts at height 7 over the
// application's state at 6, with the SLH-DSA validator set. The handshake
// skips InitChain and checks the app hash; consensus continues at 7 and 8.
//
// One real consensus State runs; the other validators are signing stubs.
// Networked reactors are not exercised, and the PQC transport stays ML-DSA.

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha512"
	"encoding/json"
	"os"
	"sync/atomic"
	"testing"
	"time"

	"github.com/cloudflare/circl/sign/slhdsa"
	"github.com/stretchr/testify/require"

	dbm "github.com/cometbft/cometbft-db"
	"github.com/cometbft/cometbft/abci/example/kvstore"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/crypto"
	cryptoenc "github.com/cometbft/cometbft/crypto/encoding"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/crypto/slhdsa192f"
	"github.com/cometbft/cometbft/internal/test"
	cmtpubsub "github.com/cometbft/cometbft/libs/pubsub"
	"github.com/cometbft/cometbft/mempool"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	"github.com/cometbft/cometbft/proxy"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/store"
	"github.com/cometbft/cometbft/types"
	cmttime "github.com/cometbft/cometbft/types/time"
)

const (
	drillSwapTxHeight = 2 // block carrying the validator updates
	drillSwapHeight   = 4 // first height the SLH-DSA set signs (tx height + 2)
	drillHaltHeight   = 6
	drillPower        = 10
)

// switchingPV is the node's own key during the swap. It signs with ML-DSA-65
// below drillSwapHeight and SLH-DSA from it. It announces the new public key
// once its last ML-DSA proposal is out, so the State refetches it at the next
// commit (finalizeCommit calls updatePrivValidatorPubKey).
type switchingPV struct {
	old, next types.PrivValidator
	announced atomic.Bool
}

func (pv *switchingPV) GetPubKey() (crypto.PubKey, error) {
	if pv.announced.Load() {
		return pv.next.GetPubKey()
	}
	return pv.old.GetPubKey()
}

func (pv *switchingPV) pick(height int64) types.PrivValidator {
	if height >= drillSwapHeight {
		return pv.next
	}
	return pv.old
}

func (pv *switchingPV) SignVote(chainID string, vote *cmtproto.Vote) error {
	return pv.pick(vote.Height).SignVote(chainID, vote)
}

func (pv *switchingPV) SignProposal(chainID string, proposal *cmtproto.Proposal) error {
	return pv.pick(proposal.Height).SignProposal(chainID, proposal)
}

type drillCheckpoint struct {
	ChainID              string `json:"chain_id"`
	HaltedHeight         int64  `json:"halted_height"`
	BlockHash            []byte `json:"block_hash"`
	AppHash              []byte `json:"app_hash"`
	NextValidatorsHash   []byte `json:"next_validators_hash"`
	RestartInitialHeight int64  `json:"restart_initial_height"`
}

func (c drillCheckpoint) digest(t *testing.T) []byte {
	raw, err := json.Marshal(c)
	require.NoError(t, err)
	sum := sha512.Sum512(raw)
	return sum[:]
}

func drillParams() *types.ConsensusParams {
	c := test.ConsensusParams()
	// The default block budget is invalid with 35,664-byte signatures (see
	// launch/CRYPTO_AGILITY_SPIKE.md); a chain must set its own.
	c.Block.MaxBytes = 8 << 20
	c.Evidence.MaxBytes = 1 << 20
	c.Validator.PubKeyTypes = []string{mldsa65.KeyType, slhdsa192f.KeyType}
	return c
}

func slhPV(t *testing.T) types.PrivValidator {
	key, err := slhdsa192f.GenPrivKey()
	require.NoError(t, err)
	return types.NewMockPVWithParams(key, false, false)
}

func pvAddress(t *testing.T, pv types.PrivValidator) []byte {
	pub, err := pv.GetPubKey()
	require.NoError(t, err)
	return pub.Address()
}

func valTx(t *testing.T, pv types.PrivValidator, power int64) []byte {
	pub, err := pv.GetPubKey()
	require.NoError(t, err)
	proto, err := cryptoenc.PubKeyToProto(pub)
	require.NoError(t, err)
	return kvstore.MakeValSetChangeTx(proto, power)
}

// drillTimeout replaces the shared 200 ms ensureTimeout: verifying SLH-DSA
// votes and extensions takes longer than that under the race detector.
const drillTimeout = 30 * time.Second

func drillNewRound(t *testing.T, ch <-chan cmtpubsub.Message, height int64) {
	t.Helper()
	select {
	case <-time.After(drillTimeout):
		t.Fatalf("no NewRound for height %d", height)
	case msg := <-ch:
		ev, ok := msg.Data().(types.EventDataNewRound)
		require.True(t, ok)
		require.Equal(t, height, ev.Height)
		require.Zero(t, ev.Round)
	}
}

func drillProposal(t *testing.T, ch <-chan cmtpubsub.Message, height int64) types.BlockID {
	t.Helper()
	select {
	case <-time.After(drillTimeout):
		t.Fatalf("no complete proposal for height %d", height)
	case msg := <-ch:
		ev, ok := msg.Data().(types.EventDataCompleteProposal)
		require.True(t, ok)
		require.Equal(t, height, ev.Height)
		require.Zero(t, ev.Round)
		return ev.BlockID
	}
	return types.BlockID{}
}

// drillHeight commits the current height. If the proposer is a stub, the
// node builds the block and the stub signs the proposal. Every stub in the
// current validator set precommits; the node's own vote is not needed.
// nextTxs enter the mempool once this height's proposal is complete, so the
// next height's block carries them even when the node itself proposes it.
func drillHeight(
	ctx context.Context,
	t *testing.T,
	cs *State,
	selfAddr func() []byte,
	stubs []*validatorStub,
	proposalCh, newRoundCh <-chan cmtpubsub.Message,
	nextTxs [][]byte,
	afterProposal func(),
) (types.BlockID, time.Duration) {
	t.Helper()
	rs := cs.GetRoundState()
	height := rs.Height
	incrementHeight(stubs...)
	proposer := rs.Validators.GetProposer()
	if !bytes.Equal(proposer.Address, selfAddr()) {
		cs.mtx.Lock()
		block, err := cs.createProposalBlock(ctx)
		cs.mtx.Unlock()
		require.NoError(t, err)
		parts, err := block.MakePartSet(types.BlockPartSizeBytes)
		require.NoError(t, err)
		blockID := types.BlockID{Hash: block.Hash(), PartSetHeader: parts.Header()}
		var signer *validatorStub
		for _, s := range stubs {
			if bytes.Equal(pvAddress(t, s), proposer.Address) {
				signer = s
			}
		}
		require.NotNil(t, signer, "proposer %X has no stub", proposer.Address)
		proposal := types.NewProposal(height, 0, -1, blockID)
		p := proposal.ToProto()
		require.NoError(t, signer.SignProposal(test.DefaultTestChainID, p))
		proposal.Signature = p.Signature
		require.NoError(t, cs.SetProposalAndBlock(proposal, block, parts, "drill"))
	}
	blockID := drillProposal(t, proposalCh, height)
	if afterProposal != nil {
		afterProposal()
	}
	for _, tx := range nextTxs {
		require.NoError(t, assertMempool(cs.txNotifier).CheckTx(tx, nil, mempool.TxInfo{}))
	}
	start := time.Now()
	for _, s := range stubs {
		index, val := rs.Validators.GetByAddress(pvAddress(t, s))
		if val == nil {
			continue
		}
		s.Index = index
		signAddVotes(cs, cmtproto.PrecommitType, blockID.Hash, blockID.PartSetHeader, true, s)
	}
	drillNewRound(t, newRoundCh, height+1)
	return blockID, time.Since(start)
}

func requireCommitKeyType(t *testing.T, cs *State, stateStore sm.Store, height int64, keyType string, sigSize int) int {
	t.Helper()
	vals, err := stateStore.LoadValidators(height)
	require.NoError(t, err)
	for _, v := range vals.Validators {
		require.Equal(t, keyType, v.PubKey.Type(), "height %d validator type", height)
	}
	meta := cs.blockStore.LoadBlockMeta(height)
	commit := cs.blockStore.LoadSeenCommit(height)
	require.NotNil(t, commit)
	signed := 0
	for _, sig := range commit.Signatures {
		if sig.BlockIDFlag == types.BlockIDFlagCommit {
			require.Len(t, sig.Signature, sigSize)
			signed++
		}
	}
	require.GreaterOrEqual(t, signed, 3)
	require.NoError(t, vals.VerifyCommit(test.DefaultTestChainID, meta.BlockID, height, commit))
	require.Equal(t, vals.Hash(), []byte(meta.Header.ValidatorsHash))
	pc := commit.ToProto()
	return pc.Size()
}

func TestLatticeBreakDrill(t *testing.T) {
	if testing.Short() {
		t.Skip("signs about 30 SLH-DSA votes")
	}
	ctx := context.Background()
	params := drillParams()

	// Genesis: four ML-DSA-65 validators. The node runs the first.
	oldPVs := make([]types.PrivValidator, 4)
	newPVs := make([]types.PrivValidator, 4)
	genVals := make([]types.GenesisValidator, 4)
	for i := range oldPVs {
		oldPVs[i] = types.NewMockPV()
		pub, err := oldPVs[i].GetPubKey()
		require.NoError(t, err)
		genVals[i] = types.GenesisValidator{PubKey: pub, Power: drillPower}
		newPVs[i] = slhPV(t)
	}
	genDoc := &types.GenesisDoc{
		GenesisTime:     cmttime.Now(),
		ChainID:         test.DefaultTestChainID,
		Validators:      genVals,
		ConsensusParams: params,
	}
	require.NoError(t, genDoc.ValidateAndComplete())
	state, err := sm.MakeGenesisState(genDoc)
	require.NoError(t, err)
	state.Version.Consensus.App = kvstore.AppVersion

	appDB := dbm.NewMemDB()
	app := kvstore.NewApplication(appDB)
	_, err = app.InitChain(ctx, &abci.RequestInitChain{Validators: types.TM2PB.ValidatorUpdates(state.Validators)})
	require.NoError(t, err)

	self := &switchingPV{old: oldPVs[0], next: newPVs[0]}
	config := ResetConfig("lattice_drill_a")
	t.Cleanup(func() { _ = os.RemoveAll(config.RootDir) })
	blockDB := dbm.NewMemDB()
	cs := newStateWithConfigAndBlockStore(config, state, self, app, blockDB)
	cs.SetTimeoutTicker(newMockTickerFunc(true)())
	stateStore := sm.NewStore(blockDB, sm.StoreOptions{})
	proposalCh := subscribe(cs.eventBus, types.EventQueryCompleteProposal)
	newRoundCh := subscribe(cs.eventBus, types.EventQueryNewRound)

	stubs := make([]*validatorStub, 0, 6)
	for i := 1; i < 4; i++ {
		stubs = append(stubs, newValidatorStub(oldPVs[i], int32(i)), newValidatorStub(newPVs[i], int32(i)))
	}
	selfAddr := func() []byte { return pvAddress(t, self) }

	swapTxs := make([][]byte, 0, 8)
	for i := range newPVs {
		swapTxs = append(swapTxs, valTx(t, newPVs[i], drillPower))
		swapTxs = append(swapTxs, valTx(t, oldPVs[i], 0))
	}

	startTestRound(cs, 1, 0)
	drillNewRound(t, newRoundCh, 1)
	var slhTimes []time.Duration
	for height := int64(1); height <= drillHaltHeight; height++ {
		var txs [][]byte
		if height == drillSwapTxHeight-1 {
			txs = swapTxs
		}
		var after func()
		if height == drillSwapHeight-1 {
			after = func() { self.announced.Store(true) }
		}
		_, took := drillHeight(ctx, t, cs, selfAddr, stubs, proposalCh, newRoundCh, txs, after)
		if height >= drillSwapHeight {
			slhTimes = append(slhTimes, took)
		}
	}

	// The updates are in the swap block; below the swap height commits are
	// ML-DSA, and from it SLH-DSA only.
	require.Len(t, cs.blockStore.LoadBlock(drillSwapTxHeight).Txs, len(swapTxs))
	mlSize := requireCommitKeyType(t, cs, stateStore, drillSwapHeight-1, mldsa65.KeyType, mldsa65.SignatureSize)
	for h := int64(drillSwapHeight); h <= drillHaltHeight; h++ {
		slhSize := requireCommitKeyType(t, cs, stateStore, h, slhdsa192f.KeyType, slhdsa192f.SignatureSize)
		t.Logf("height %d: SLH-DSA commit %d bytes (ML-DSA commit at %d: %d bytes); 3 stub votes signed and verified in %v",
			h, slhSize, drillSwapHeight-1, mlSize, slhTimes[h-drillSwapHeight])
	}

	// Equivocation evidence: two conflicting prevotes from one validator. The
	// approved production evidence budget (engine_evidence_max_bytes, P01
	// E05 values 4) is 65,536 bytes, and the evidence pool stops at the first
	// item that does not fit, so oversized evidence is never proposed.
	evidenceSize := func(stub *validatorStub, height int64) int {
		vals, err := stateStore.LoadValidators(height)
		require.NoError(t, err)
		index, _ := vals.GetByAddress(pvAddress(t, stub))
		stub.Index, stub.Height, stub.Round = index, height, 0
		a := signVote(stub, cmtproto.PrevoteType, bytes.Repeat([]byte{1}, 32), types.PartSetHeader{Total: 1, Hash: bytes.Repeat([]byte{2}, 32)}, false)
		b := signVote(stub, cmtproto.PrevoteType, bytes.Repeat([]byte{3}, 32), types.PartSetHeader{Total: 1, Hash: bytes.Repeat([]byte{4}, 32)}, false)
		ev, err := types.NewDuplicateVoteEvidence(a, b, cmttime.Now(), vals)
		require.NoError(t, err)
		require.NoError(t, ev.ValidateBasic())
		pb, err := types.EvidenceToProto(ev)
		require.NoError(t, err)
		list := cmtproto.EvidenceList{Evidence: []cmtproto.Evidence{*pb}}
		return list.Size()
	}
	mlEvidence := evidenceSize(stubs[0], drillSwapHeight-1)
	slhEvidence := evidenceSize(stubs[1], drillHaltHeight)
	t.Logf("duplicate-vote evidence: ML-DSA-65 %d bytes, SLH-DSA-SHAKE-192f %d bytes (production budget 65536)", mlEvidence, slhEvidence)
	require.Less(t, mlEvidence, 65536)
	require.Greater(t, slhEvidence, 65536, "SLH-DSA equivocation evidence exceeds the approved evidence budget")
	for _, s := range stubs {
		s.Height, s.Round = drillHaltHeight, 0
	}

	// Halt after drillHaltHeight; a root key signs the restart checkpoint.
	halted := cs.GetState()
	require.Equal(t, int64(drillHaltHeight), halted.LastBlockHeight)
	slhVals := halted.NextValidators.Copy()
	for _, v := range slhVals.Validators {
		require.Equal(t, slhdsa192f.KeyType, v.PubKey.Type())
	}
	checkpoint := drillCheckpoint{
		ChainID:              halted.ChainID,
		HaltedHeight:         drillHaltHeight,
		BlockHash:            halted.LastBlockID.Hash,
		AppHash:              halted.AppHash,
		NextValidatorsHash:   slhVals.Hash(),
		RestartInitialHeight: drillHaltHeight + 1,
	}
	rootPub, rootPriv, err := slhdsa.GenerateKey(rand.Reader, slhdsa.SHAKE_256s)
	require.NoError(t, err)
	started := time.Now()
	rootSig, err := slhdsa.SignDeterministic(&rootPriv, slhdsa.NewMessage(checkpoint.digest(t)), []byte("dytallix-restart-checkpoint"))
	require.NoError(t, err)
	t.Logf("root checkpoint: SLH-DSA-SHAKE-256s signature %d bytes in %v", len(rootSig), time.Since(started))
	require.True(t, slhdsa.Verify(&rootPub, slhdsa.NewMessage(checkpoint.digest(t)), rootSig, []byte("dytallix-restart-checkpoint")))
	forged := checkpoint
	forged.NextValidatorsHash = bytes.Repeat([]byte{1}, 32)
	require.False(t, slhdsa.Verify(&rootPub, slhdsa.NewMessage(forged.digest(t)), rootSig, []byte("dytallix-restart-checkpoint")))

	// Part B: replacement genesis at the checkpoint, over the app's state.
	restartVals := make([]types.GenesisValidator, len(slhVals.Validators))
	for i, v := range slhVals.Validators {
		restartVals[i] = types.GenesisValidator{PubKey: v.PubKey, Power: v.VotingPower}
	}
	restartGen := func(appHash []byte) *types.GenesisDoc {
		g := &types.GenesisDoc{
			GenesisTime:     cmttime.Now(),
			ChainID:         checkpoint.ChainID,
			InitialHeight:   checkpoint.RestartInitialHeight,
			Validators:      restartVals,
			ConsensusParams: params,
			AppHash:         appHash,
		}
		require.NoError(t, g.ValidateAndComplete())
		return g
	}
	handshake := func(g *types.GenesisDoc) (sm.State, dbm.DB, abci.Application) {
		restartState, err := sm.MakeGenesisState(g)
		require.NoError(t, err)
		db := dbm.NewMemDB()
		ss := sm.NewStore(db, sm.StoreOptions{})
		require.NoError(t, ss.Save(restartState))
		restartApp := kvstore.NewApplication(appDB) // a fresh process over the same app data
		conns := proxy.NewAppConns(proxy.NewLocalClientCreator(restartApp), proxy.NopMetrics())
		require.NoError(t, conns.Start())
		defer func() { _ = conns.Stop() }()
		require.NoError(t, NewHandshaker(ss, restartState, store.NewBlockStore(db), g).Handshake(conns))
		restartState.Version.Consensus.App = kvstore.AppVersion
		return restartState, db, restartApp
	}

	// A genesis whose app hash is not the checkpoint's is refused.
	require.Panics(t, func() { handshake(restartGen(bytes.Repeat([]byte{9}, 32))) })

	restartState, restartDB, restartApp := handshake(restartGen(checkpoint.AppHash))
	config2 := ResetConfig("lattice_drill_b")
	t.Cleanup(func() { _ = os.RemoveAll(config2.RootDir) })
	cs2 := newStateWithConfigAndBlockStore(config2, restartState, newPVs[0], restartApp, restartDB)
	cs2.SetTimeoutTicker(newMockTickerFunc(true)())
	stateStore2 := sm.NewStore(restartDB, sm.StoreOptions{})
	proposalCh2 := subscribe(cs2.eventBus, types.EventQueryCompleteProposal)
	newRoundCh2 := subscribe(cs2.eventBus, types.EventQueryNewRound)
	slhStubs := []*validatorStub{stubs[1], stubs[3], stubs[5]}
	startTestRound(cs2, drillHaltHeight+1, 0)
	drillNewRound(t, newRoundCh2, drillHaltHeight+1)
	selfAddr2 := func() []byte { return pvAddress(t, newPVs[0]) }
	for h := int64(drillHaltHeight + 1); h <= drillHaltHeight+2; h++ {
		drillHeight(ctx, t, cs2, selfAddr2, slhStubs, proposalCh2, newRoundCh2, nil, nil)
		requireCommitKeyType(t, cs2, stateStore2, h, slhdsa192f.KeyType, slhdsa192f.SignatureSize)
	}
	first := cs2.blockStore.LoadBlock(drillHaltHeight + 1)
	// The restarted chain continues the application state, but its first block
	// cannot name block 6: the hash chain breaks and only the checkpoint links them.
	require.Equal(t, checkpoint.AppHash, []byte(first.AppHash))
	require.True(t, first.LastBlockID.IsZero())
	require.Equal(t, int64(drillHaltHeight+2), cs2.GetState().LastBlockHeight)
}
