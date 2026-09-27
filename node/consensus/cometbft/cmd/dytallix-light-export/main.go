// Writes the light blocks a joining node's state sync verifies, from the
// stores of a node the operator runs (Dytallix state sync v1, rule 5). It
// only reads the stores; stop the node first, or use a copy of its home.
package main

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"dytallix.local/consensus/cometbft/internal/lightblocks"
	cfg "github.com/cometbft/cometbft/config"
	sm "github.com/cometbft/cometbft/state"
	"github.com/cometbft/cometbft/store"
	"github.com/cometbft/cometbft/types"
)

func run(args []string) error {
	flags := flag.NewFlagSet("dytallix-light-export", flag.ContinueOnError)
	home := flags.String("home", "", "engine home of the exporting node")
	from := flags.Int64("from", 0, "first height: the joining node's trusted height")
	to := flags.Int64("to", 0, "last height: at least the snapshot height plus two")
	output := flags.String("output", "", "new, empty directory for the export")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 || *home == "" || *output == "" || !filepath.IsAbs(*output) {
		return errors.New("usage: dytallix-light-export --home HOME --from H --to H --output ABSOLUTE_DIR")
	}
	if err := os.Mkdir(*output, 0o700); err != nil {
		return fmt.Errorf("output must be a new directory: %w", err)
	}
	config, err := enginepqc.ReadConfig(*home)
	if err != nil {
		return err
	}
	genesis, err := types.GenesisDocFromFile(config.GenesisFile())
	if err != nil {
		return err
	}
	blockDB, err := cfg.DefaultDBProvider(&cfg.DBContext{ID: "blockstore", Config: config})
	if err != nil {
		return err
	}
	defer blockDB.Close()
	stateDB, err := cfg.DefaultDBProvider(&cfg.DBContext{ID: "state", Config: config})
	if err != nil {
		return err
	}
	defer stateDB.Close()
	blocks := store.NewBlockStore(blockDB)
	states := sm.NewStore(stateDB, sm.StoreOptions{})
	if err = lightblocks.Export(blocks, states, genesis.ChainID, *from, *to, *output); err != nil {
		return err
	}
	trusted := blocks.LoadBlockMeta(*from)
	// The operator configures the joining node to trust this header.
	return json.NewEncoder(os.Stdout).Encode(map[string]any{
		"chain_id": genesis.ChainID, "from": *from, "to": *to,
		"trust_height": *from, "trust_hash": hex.EncodeToString(trusted.BlockID.Hash),
	})
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
