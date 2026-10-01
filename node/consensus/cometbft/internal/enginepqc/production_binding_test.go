package enginepqc

import (
	"strings"
	"testing"
)

func TestProductionBindingCodecIsCanonical(t *testing.T) {
	digest := strings.Repeat("ab", 32)
	raw := `{"schema":1,"role":"sentry","chain_id":"c","config_sha256":"` + digest + `","genesis_sha256":"` + digest +
		`","transport_sha256":"` + digest + `","peer_public_key_sha256":"` + digest + `","validator_public_key_sha256":"` + digest + `"}`
	binding, err := DecodeProductionBinding([]byte(raw))
	if err != nil || binding.Role != RoleSentry || binding.ChainID != "c" {
		t.Fatalf("canonical binding refused: %v", err)
	}
	for name, changed := range map[string]string{
		"spaced":   strings.Replace(raw, `"schema":1`, `"schema": 1`, 1),
		"unknown":  strings.Replace(raw, `{"schema":1`, `{"schema":1,"note":"x"`, 1),
		"trailing": raw + `{}`,
		"reorder":  strings.Replace(raw, `"schema":1,"role":"sentry"`, `"role":"sentry","schema":1`, 1),
	} {
		if _, err := DecodeProductionBinding([]byte(changed)); err == nil {
			t.Fatalf("%s binding accepted", name)
		}
	}
	if ValidateProductionBinding(nil, binding) == nil {
		t.Fatal("binding without a production runtime accepted")
	}
}

func TestDevelopmentBuildRefusesTheProductionProfile(t *testing.T) {
	if ProductionBuild {
		t.Skip("development build only")
	}
	if _, err := Load("/nonexistent", ProductionProfile); err == nil || !strings.Contains(err.Error(), "no production transport profile") {
		t.Fatalf("production profile in a development build: %v", err)
	}
}
