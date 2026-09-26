//go:build dytallix_pqc_only

package enginepqc

import (
	"bytes"
	cfg "github.com/cometbft/cometbft/config"
	"github.com/spf13/viper"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func legacyConfigForComparison(raw []byte) (*cfg.Config, error) {
	v := viper.New()
	v.SetConfigType("toml")
	if err := v.ReadConfig(bytes.NewReader(raw)); err != nil {
		return nil, err
	}
	c := cfg.DefaultConfig()
	if err := v.UnmarshalExact(c); err != nil {
		return nil, err
	}
	return c, nil
}
func TestPQCConfigDecoderMatchesExistingFixture(t *testing.T) {
	c := isolatedConfig(t)
	c.Consensus.TimeoutPropose = 2100 * time.Millisecond
	c.RPC.CORSAllowedOrigins = []string{"http://127.0.0.1:4173"}
	path := filepath.Join(t.TempDir(), "config.toml")
	cfg.WriteConfigFile(path, c)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	got, err := decodeConfig(raw)
	if err != nil {
		t.Fatal(err)
	}
	want, err := legacyConfigForComparison(raw)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatal("complete fixture configuration differs from legacy decoder")
	}
}
func TestPQCConfigDecoderPreservesDefaultsConversionsAndErrors(t *testing.T) {
	cases := []string{
		"", `MONIKER="mixed-case"`, "[consensus]\ntimeout_propose=\"2.5s\"\n", "[rpc]\nmax_open_connections=\"19\"\ncors_allowed_methods=\"HEAD,GET,POST\"\n", "[rpc]\nunknown_option=1\n", "unknown_option=1\n", "[consensus]\ntimeout_propose=\"invalid\"\n", "moniker=\"one\"\nmoniker=\"two\"\n",
	}
	for _, raw := range cases {
		got, err := decodeConfig([]byte(raw))
		want, legacyErr := legacyConfigForComparison([]byte(raw))
		if (err == nil) != (legacyErr == nil) {
			t.Fatalf("acceptance changed for %q: new=%v legacy=%v", raw, err, legacyErr)
		}
		if err == nil && !reflect.DeepEqual(got, want) {
			t.Fatalf("configuration changed for %q", raw)
		}
	}
}
func TestPQCConfigDecoderRejectsAmbiguousCaseCollision(t *testing.T) {
	if _, err := decodeConfig([]byte("moniker=\"one\"\nMONIKER=\"two\"\n")); err == nil {
		t.Fatal("case-colliding keys accepted")
	}
}
