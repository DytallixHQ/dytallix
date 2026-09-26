package main

import (
	"encoding/json"
	"testing"
)

func TestPenaltyFixtureRequiresLifecycleInputs(t *testing.T) {
	if _, err := generateWithPenalty("", "", "fixture", "", 28650, "", true); err == nil {
		t.Fatal("penalty fixture accepted missing lifecycle operators")
	}
}

func TestPenaltyFixtureUsesExplicitLocalOnlyParameters(t *testing.T) {
	p := fixturePenalty("fixture")
	raw, err := json.Marshal(p)
	if err != nil {
		t.Fatal(err)
	}
	expected := `{"version":1,"profile":"cometbft-penalty-local-qualification","chain_id":"fixture","penalty_numerator":1,"penalty_denominator":20,"production_activation":false}`
	if string(raw) != expected {
		t.Fatalf("unexpected local penalty policy: %s", raw)
	}
	old, err := json.Marshal(applicationConfig{})
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(old, &fields); err != nil {
		t.Fatal(err)
	}
	if _, exists := fields["penalty"]; exists {
		t.Fatal("legacy fixture gained penalty configuration")
	}
}
