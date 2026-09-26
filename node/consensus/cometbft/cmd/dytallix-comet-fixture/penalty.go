package main

// These values apply only to disposable local fixtures. They are not approved
// production penalty parameters. Principal enters a reserve with no sweep.
type penaltyConfig struct {
	Version              uint32 `json:"version"`
	Profile              string `json:"profile"`
	ChainID              string `json:"chain_id"`
	PenaltyNumerator     uint64 `json:"penalty_numerator"`
	PenaltyDenominator   uint64 `json:"penalty_denominator"`
	ProductionActivation bool   `json:"production_activation"`
}

func fixturePenalty(chainID string) *penaltyConfig {
	return &penaltyConfig{Version: 1, Profile: "cometbft-penalty-local-qualification", ChainID: chainID, PenaltyNumerator: 1, PenaltyDenominator: 20, ProductionActivation: false}
}
