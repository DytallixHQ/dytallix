// Package lightblocks gives a joining node's light client the light blocks
// an operator exported from a node they run (Dytallix state sync v1, rule 5).
// The light client verifies every header and commit with ML-DSA-65 from the
// operator's trusted height and hash, so the files are trusted only for
// availability. No network protocol is added.
//
// Layout of an export directory: {height:020}.block holds a LightBlock
// protobuf, and {height:020}.params the ConsensusParams protobuf in effect at
// that height, which the state provider checks against the header's
// consensus hash.
package lightblocks

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/cometbft/cometbft/light/provider"
	cmtproto "github.com/cometbft/cometbft/proto/tendermint/types"
	"github.com/cometbft/cometbft/types"
)

// A light block or parameter file is bounded when read back.
const maxFileBytes = 16 << 20

func fileName(height int64, suffix string) string {
	return fmt.Sprintf("%020d.%s", height, suffix)
}

// writeFile writes data under dir atomically: a private temporary file, then
// a rename.
func writeFile(dir, name string, data []byte) error {
	file, err := os.CreateTemp(dir, ".tmp-"+name+"-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return os.Rename(file.Name(), filepath.Join(dir, name))
}

func readFile(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, maxFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxFileBytes {
		return nil, errors.New("light block file exceeds its bound")
	}
	return data, nil
}

// Write stores a light block of chainID, and the consensus parameters in
// effect at its height when params is not nil.
func Write(dir, chainID string, block *types.LightBlock, params *types.ConsensusParams) error {
	if block == nil {
		return errors.New("missing light block")
	}
	if err := block.ValidateBasic(chainID); err != nil {
		return err
	}
	pb, err := block.ToProto()
	if err != nil {
		return err
	}
	data, err := pb.Marshal()
	if err != nil {
		return err
	}
	if err = writeFile(dir, fileName(block.Height, "block"), data); err != nil {
		return err
	}
	if params == nil {
		return nil
	}
	if err = params.ValidateBasic(); err != nil {
		return err
	}
	pp := params.ToProto()
	data, err = pp.Marshal()
	if err != nil {
		return err
	}
	return writeFile(dir, fileName(block.Height, "params"), data)
}

// Provider serves the light blocks of one export directory. It implements
// the light client's provider interface.
type Provider struct {
	chainID string
	dir     string
}

var _ provider.Provider = (*Provider)(nil)

func NewProvider(chainID, dir string) (*Provider, error) {
	if chainID == "" || !filepath.IsAbs(dir) {
		return nil, errors.New("light block export requires a chain and an absolute directory")
	}
	stat, err := os.Stat(dir)
	if err != nil {
		return nil, err
	}
	if !stat.IsDir() {
		return nil, errors.New("light block export is not a directory")
	}
	return &Provider{chainID: chainID, dir: dir}, nil
}

func (p *Provider) ChainID() string { return p.chainID }
func (p *Provider) String() string  { return "light-blocks:" + p.dir }

// latest is the highest exported height, or zero.
func (p *Provider) latest() (int64, error) {
	entries, err := os.ReadDir(p.dir)
	if err != nil {
		return 0, err
	}
	var latest int64
	for _, entry := range entries {
		name, ok := strings.CutSuffix(entry.Name(), ".block")
		if !ok || len(name) != 20 {
			continue
		}
		height, err := strconv.ParseInt(name, 10, 64)
		if err == nil && fileName(height, "block") == entry.Name() && height > latest {
			latest = height
		}
	}
	return latest, nil
}

// LightBlock returns the exported light block at height, or the latest for
// height zero. It checks the block's form only; the light client verifies it.
func (p *Provider) LightBlock(_ context.Context, height int64) (*types.LightBlock, error) {
	if height < 0 {
		return nil, provider.ErrLightBlockNotFound
	}
	if height == 0 {
		latest, err := p.latest()
		if err != nil || latest == 0 {
			return nil, provider.ErrLightBlockNotFound
		}
		height = latest
	}
	data, err := readFile(filepath.Join(p.dir, fileName(height, "block")))
	if errors.Is(err, os.ErrNotExist) {
		return nil, provider.ErrLightBlockNotFound
	}
	if err != nil {
		return nil, provider.ErrBadLightBlock{Reason: err}
	}
	var pb cmtproto.LightBlock
	if err = pb.Unmarshal(data); err != nil {
		return nil, provider.ErrBadLightBlock{Reason: err}
	}
	block, err := types.LightBlockFromProto(&pb)
	if err != nil {
		return nil, provider.ErrBadLightBlock{Reason: err}
	}
	if block.Height != height {
		return nil, provider.ErrBadLightBlock{Reason: errors.New("light block height differs from its file")}
	}
	if err = block.ValidateBasic(p.chainID); err != nil {
		return nil, provider.ErrBadLightBlock{Reason: err}
	}
	return block, nil
}

// ReportEvidence has no network to report to. The light client still stops
// verifying when it detects a conflicting header.
func (p *Provider) ReportEvidence(context.Context, types.Evidence) error { return nil }

// ConsensusParams returns the exported parameters in effect at height,
// unverified: the caller checks them against a verified header.
func (p *Provider) ConsensusParams(height int64) (types.ConsensusParams, error) {
	data, err := readFile(filepath.Join(p.dir, fileName(height, "params")))
	if err != nil {
		return types.ConsensusParams{}, err
	}
	var pb cmtproto.ConsensusParams
	if err = pb.Unmarshal(data); err != nil {
		return types.ConsensusParams{}, err
	}
	params := types.ConsensusParamsFromProto(pb)
	if err = params.ValidateBasic(); err != nil {
		return types.ConsensusParams{}, err
	}
	return params, nil
}
