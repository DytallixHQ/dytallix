package main

import (
	"bytes"
	"crypto/sha3"
	"encoding/base64"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"dytallix.local/consensus/cometbft/internal/enginepqc"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/types"
)

var (
	preparation = filepath.Join("..", "..", "..", "..", "tools", "mainnet-preparation", "fixtures")
	rehearsal   = filepath.Join(preparation, "host-config-rehearsal")
	genesisPath = filepath.Join(preparation, "genesis-production-rehearsal", "genesis.json")
)

// syntheticKey is SHAKE-256 bytes in the shape of an ML-DSA-65 public key,
// like the genesis rehearsal's validator keys: no private key exists for it.
func syntheticKey(kind, label string) string {
	raw := sha3.SumSHAKE256([]byte("DYTALLIX/HOST-CONFIG-REHEARSAL/v1\x00"+kind+"/"+label), mldsa65.PubKeySize)
	return base64.StdEncoding.EncodeToString(raw)
}

func read(t *testing.T, path string) []byte {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// fixturePlan is the synthetic staging pin plan for the production-profile
// genesis rehearsal: four operators, each with a validator and its sentries,
// and one endpoint. Documentation addresses (RFC 5737) only.
func fixturePlan(t *testing.T) plan {
	t.Helper()
	genesis, err := types.GenesisDocFromJSON(read(t, genesisPath))
	if err != nil {
		t.Fatal(err)
	}
	channel, status := "203.0.113.11:9443", "203.0.113.11:8080"
	hosts := []host{
		{Label: "validator-1", Operator: "operator-1", Role: "validator", P2P: "192.0.2.11:26656", Pins: []string{"sentry-1a", "sentry-1b"}},
		{Label: "validator-2", Operator: "operator-2", Role: "validator", P2P: "192.0.2.12:26656", Pins: []string{"sentry-2"}},
		{Label: "validator-3", Operator: "operator-3", Role: "validator", P2P: "192.0.2.13:26656", Pins: []string{"sentry-3"}},
		{Label: "validator-4", Operator: "operator-4", Role: "validator", P2P: "192.0.2.14:26656", Pins: []string{"sentry-4"}},
		{Label: "sentry-1a", Operator: "operator-1", Role: "sentry", P2P: "198.51.100.11:26656", Pins: []string{"validator-1", "sentry-1b", "sentry-2", "sentry-3", "sentry-4", "endpoint-1"}},
		{Label: "sentry-1b", Operator: "operator-1", Role: "sentry", P2P: "198.51.100.12:26656", Pins: []string{"validator-1", "sentry-1a", "sentry-2", "sentry-3", "sentry-4"}},
		{Label: "sentry-2", Operator: "operator-2", Role: "sentry", P2P: "198.51.100.21:26656", Pins: []string{"validator-2", "sentry-1a", "sentry-1b", "sentry-3", "sentry-4", "endpoint-1"}},
		{Label: "sentry-3", Operator: "operator-3", Role: "sentry", P2P: "198.51.100.31:26656", Pins: []string{"validator-3", "sentry-1a", "sentry-1b", "sentry-2", "sentry-4", "endpoint-1"}},
		{Label: "sentry-4", Operator: "operator-4", Role: "sentry", P2P: "198.51.100.41:26656", Pins: []string{"validator-4", "sentry-1a", "sentry-1b", "sentry-2", "sentry-3", "endpoint-1"}},
		{Label: "endpoint-1", Operator: "operator-1", Role: "endpoint", P2P: "203.0.113.11:26656", Channel: &channel, Status: &status, Pins: []string{"sentry-1a", "sentry-2", "sentry-3", "sentry-4"}},
	}
	for i := range hosts {
		h := &hosts[i]
		h.Home = "/srv/dytallix/node"
		h.PeerPublicKey = syntheticKey("peer", h.Label)
		h.ValidatorPublicKey = syntheticKey("validator", h.Label)
		if h.Role == enginepqc.RoleValidator {
			h.ValidatorPublicKey = base64.StdEncoding.EncodeToString(genesis.Validators[i].PubKey.Bytes())
		}
	}
	return plan{Schema: planSchema, ChainID: genesis.ChainID, Hosts: hosts}
}

func encodePlan(t *testing.T, p plan) []byte {
	t.Helper()
	raw, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	return append(raw, '\n')
}

func generateFixture(t *testing.T, planRaw []byte) ([]output, error) {
	t.Helper()
	return generate(planRaw, read(t, filepath.Join(rehearsal, "host-values.json")), read(t, genesisPath), t.TempDir())
}

func filesByPath(files []output) map[string][]byte {
	byPath := map[string][]byte{}
	for _, file := range files {
		byPath[file.path] = file.data
	}
	return byPath
}

// The committed plan and bindings are reproducible. Set
// DYTALLIX_UPDATE_HOST_FIXTURE=1 to rewrite them after an intended change.
func TestTheCommittedRehearsalIsCurrent(t *testing.T) {
	planRaw := encodePlan(t, fixturePlan(t))
	files, err := generateFixture(t, planRaw)
	if err != nil {
		t.Fatal(err)
	}
	bindingsRaw := filesByPath(files)["PIN_PLAN_BINDINGS.json"]
	if os.Getenv("DYTALLIX_UPDATE_HOST_FIXTURE") == "1" {
		for name, data := range map[string][]byte{"PIN_PLAN.json": planRaw, "PIN_PLAN_BINDINGS.json": bindingsRaw} {
			if err := os.WriteFile(filepath.Join(rehearsal, name), data, 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
	if !bytes.Equal(planRaw, read(t, filepath.Join(rehearsal, "PIN_PLAN.json"))) {
		t.Fatal("PIN_PLAN.json is stale; rerun with DYTALLIX_UPDATE_HOST_FIXTURE=1")
	}
	if !bytes.Equal(bindingsRaw, read(t, filepath.Join(rehearsal, "PIN_PLAN_BINDINGS.json"))) {
		t.Fatal("PIN_PLAN_BINDINGS.json is stale; rerun with DYTALLIX_UPDATE_HOST_FIXTURE=1")
	}
	var summary bindings
	if err := strict(bindingsRaw, &summary); err != nil || len(summary.Hosts) != 10 || summary.ChainID != "dytallix-staging-1" {
		t.Fatalf("bindings summary: %v", err)
	}
	for _, h := range summary.Hosts {
		raw, _ := json.Marshal(h.Binding)
		if h.BindingSHA256 != digest(raw) || h.Firewall.TransportSHA256 != h.Binding.TransportSHA256 {
			t.Fatalf("%s: summary digests differ from its binding", h.Label)
		}
		if (h.Role == enginepqc.RoleEndpoint) != (len(h.Firewall.PublicListeners) == 2) {
			t.Fatalf("%s: public listeners", h.Label)
		}
	}
}

func TestEveryHostGetsItsRoleAndApprovedValues(t *testing.T) {
	files, err := generateFixture(t, encodePlan(t, fixturePlan(t)))
	if err != nil {
		t.Fatal(err)
	}
	byPath := filesByPath(files)
	if len(byPath) != 31 {
		t.Fatalf("%d files", len(byPath))
	}
	for label, check := range map[string]string{"validator-1": "0", "sentry-1a": "0", "endpoint-1": "0"} {
		config := string(byPath[filepath.Join("hosts", label, "config.toml")])
		for _, want := range []string{
			"double_sign_check_height = " + check + "\n",
			`moniker = "` + label + `"`,
			`proxy_app = "unix:///srv/dytallix/node/abci/app.sock"`,
			`laddr = "tcp://127.0.0.1:26657"`,
			"pex = false\n",
			`seeds = ""`,
			`external_address = ""`,
			"addr_book_strict = true\n",
			"allow_duplicate_ip = false\n",
			`persistent_peers_max_dial_period = "1m0s"`,
			`timeout_commit = "4s"`,
			"prometheus = false\n",
			`rpc_servers = ""`,
			"send_rate = 5120000\n",
		} {
			if !strings.Contains(config, want) {
				t.Fatalf("%s config lacks %q", label, want)
			}
		}
		var tc enginepqc.TransportConfig
		if err := strict(byPath[filepath.Join("hosts", label, "pqc_transport.json")], &tc); err != nil || tc.Profile != enginepqc.ProductionProfile || tc.HandshakeTimeoutMS != 5000 {
			t.Fatalf("%s transport: %v", label, err)
		}
		binding, err := enginepqc.DecodeProductionBinding(byPath[filepath.Join("hosts", label, "binding.json")])
		if err != nil || binding.Role != strings.Split(label, "-")[0] {
			t.Fatalf("%s binding: %v %v", label, binding, err)
		}
	}
	validator := string(byPath[filepath.Join("hosts", "validator-1", "config.toml")])
	peers := fixturePlan(t).Hosts
	for _, other := range []string{"sentry-1a", "sentry-1b"} {
		for _, h := range peers {
			raw, _ := base64.StdEncoding.DecodeString(h.PeerPublicKey)
			key, _ := mldsa65.NewPubKeyFromBytes(raw)
			if h.Label == other && !strings.Contains(validator, string(p2p.PubKeyToID(key))+"@"+h.P2P) {
				t.Fatalf("validator-1 does not pin %s", other)
			}
		}
	}
}

func TestTheOutputIsDeterministicOwnerOnlyAndNeverReplaced(t *testing.T) {
	work := t.TempDir()
	planPath := filepath.Join(work, "plan.json")
	if err := os.WriteFile(planPath, encodePlan(t, fixturePlan(t)), 0o600); err != nil {
		t.Fatal(err)
	}
	args := func(out string) []string {
		return []string{"--plan", planPath, "--values", filepath.Join(rehearsal, "host-values.json"), "--genesis", genesisPath, "--out", out}
	}
	first, second := filepath.Join(work, "a"), filepath.Join(work, "b")
	for _, out := range []string{first, second} {
		if err := run(args(out), io.Discard); err != nil {
			t.Fatal(err)
		}
	}
	count := 0
	err := filepath.WalkDir(first, func(path string, entry os.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		count++
		info, _ := entry.Info()
		relative, _ := filepath.Rel(first, path)
		if info.Mode().Perm() != 0o600 || !bytes.Equal(read(t, path), read(t, filepath.Join(second, relative))) {
			t.Fatalf("%s: mode %v or bytes differ between runs", relative, info.Mode())
		}
		return nil
	})
	if err != nil || count != 31 {
		t.Fatalf("walked %d files: %v", count, err)
	}
	if err := run(args(first), io.Discard); err == nil {
		t.Fatal("an existing output directory was reused")
	}
	// Nothing is written for a refused plan.
	bad := fixturePlan(t)
	bad.Hosts[0].Pins = []string{"sentry-2"}
	if err := os.WriteFile(planPath, encodePlan(t, bad), 0o600); err != nil {
		t.Fatal(err)
	}
	refused := filepath.Join(work, "refused")
	if err := run(args(refused), io.Discard); err == nil {
		t.Fatal("refused plan generated")
	}
	if _, err := os.Lstat(refused); !os.IsNotExist(err) {
		t.Fatal("a refused plan left an output directory")
	}
	if err := run([]string{"--plan", planPath}, io.Discard); err == nil || !strings.Contains(err.Error(), "usage") {
		t.Fatalf("partial arguments: %v", err)
	}
}

func TestThePlanFollowsTheApprovedMeshRules(t *testing.T) {
	label := func(p *plan, name string) *host {
		for i := range p.Hosts {
			if p.Hosts[i].Label == name {
				return &p.Hosts[i]
			}
		}
		t.Fatalf("no host %s", name)
		return nil
	}
	repin := func(p *plan, from, to string) {
		h := label(p, from)
		for i, pin := range h.Pins {
			if pin == to {
				h.Pins = append(h.Pins[:i], h.Pins[i+1:]...)
				return
			}
		}
		h.Pins = append(h.Pins, to)
	}
	both := func(p *plan, a, b string) { repin(p, a, b); repin(p, b, a) }
	for name, tc := range map[string]struct {
		change func(*plan)
		want   string
	}{
		"chain":                {func(p *plan) { p.ChainID = "another-chain" }, "chain differs"},
		"schema":               {func(p *plan) { p.Schema = "dytallix.pin-plan.v0" }, "chain differs"},
		"shared IP":            {func(p *plan) { label(p, "sentry-2").P2P = "198.51.100.11:26657" }, "shares an IP"},
		"non-canonical":        {func(p *plan) { label(p, "sentry-2").P2P = "198.51.100.021:26656" }, "global unicast"},
		"hostname":             {func(p *plan) { label(p, "sentry-2").P2P = "sentry.example:26656" }, "global unicast"},
		"low port":             {func(p *plan) { label(p, "sentry-2").P2P = "198.51.100.21:443" }, "global unicast"},
		"loopback":             {func(p *plan) { label(p, "sentry-2").P2P = "127.0.0.2:26656" }, "global unicast"},
		"address family":       {func(p *plan) { label(p, "sentry-2").P2P = "[2001:db8::21]:26656" }, "address families"},
		"shared peer key":      {func(p *plan) { label(p, "sentry-2").PeerPublicKey = label(p, "sentry-3").PeerPublicKey }, "reuses a key of sentry-2"},
		"one key twice":        {func(p *plan) { label(p, "sentry-2").ValidatorPublicKey = label(p, "sentry-2").PeerPublicKey }, "reuses a key of sentry-2"},
		"short key":            {func(p *plan) { label(p, "sentry-2").PeerPublicKey = "AAAA" }, "canonical base64"},
		"sentry signs":         {func(p *plan) { label(p, "sentry-2").ValidatorPublicKey = label(p, "validator-2").ValidatorPublicKey }, "reuses a key of validator-2"},
		"genesis host missing": {func(p *plan) { label(p, "validator-4").Role = "sentry" }, "genesis validator set"},
		"other sentry":         {func(p *plan) { both(p, "validator-1", "sentry-2") }, "own operator's sentries"},
		"validator pair":       {func(p *plan) { both(p, "validator-1", "validator-2") }, "own operator's sentries"},
		"validator endpoint":   {func(p *plan) { both(p, "validator-1", "endpoint-1") }, "own operator's sentries"},
		"endpoint validator":   {func(p *plan) { both(p, "endpoint-1", "validator-2") }, "own operator's sentries"},
		"endpoint pair": {func(p *plan) {
			c := "203.0.113.12:9443"
			p.Hosts = append(p.Hosts, host{Label: "endpoint-2", Operator: "operator-2", Role: "endpoint", Home: "/srv/dytallix/node",
				P2P: "203.0.113.12:26656", Channel: &c, PeerPublicKey: syntheticKey("peer", "endpoint-2"),
				ValidatorPublicKey: syntheticKey("validator", "endpoint-2"), Pins: []string{"endpoint-1"}})
			repin(p, "endpoint-1", "endpoint-2")
		}, "an endpoint pins only sentries"},
		"asymmetric":          {func(p *plan) { repin(p, "sentry-1a", "sentry-2") }, "does not pin it back"},
		"self":                {func(p *plan) { label(p, "sentry-2").Pins = append(label(p, "sentry-2").Pins, "sentry-2") }, "self or repeated"},
		"unknown pin":         {func(p *plan) { repin(p, "sentry-2", "nobody") }, "unknown"},
		"no pins":             {func(p *plan) { label(p, "validator-2").Pins = nil; repin(p, "sentry-2", "validator-2") }, "1 to 64 pins"},
		"duplicate label":     {func(p *plan) { label(p, "sentry-1b").Label = "sentry-1a" }, "duplicate label"},
		"bad label":           {func(p *plan) { label(p, "sentry-4").Operator = "operator 4" }, "identifiers"},
		"role":                {func(p *plan) { label(p, "sentry-4").Role = "observer" }, "role must be"},
		"relative home":       {func(p *plan) { label(p, "sentry-4").Home = "srv/dytallix" }, "clean absolute"},
		"sentry channel":      {func(p *plan) { c := "198.51.100.21:9443"; label(p, "sentry-2").Channel = &c }, "channel listener"},
		"endpoint no channel": {func(p *plan) { label(p, "endpoint-1").Channel = nil }, "channel listener"},
		"channel elsewhere":   {func(p *plan) { c := "203.0.113.12:9443"; label(p, "endpoint-1").Channel = &c }, "P2P address on another port"},
		"channel same port":   {func(p *plan) { c := "203.0.113.11:26656"; label(p, "endpoint-1").Channel = &c }, "P2P address on another port"},
		"sentry status":       {func(p *plan) { st := "198.51.100.21:8080"; label(p, "sentry-2").Status = &st }, "only an endpoint serves the status page"},
		"status elsewhere":    {func(p *plan) { st := "203.0.113.12:8080"; label(p, "endpoint-1").Status = &st }, "status page uses the node's P2P address"},
		"status on channel":   {func(p *plan) { st := "203.0.113.11:9443"; label(p, "endpoint-1").Status = &st }, "status page uses the node's P2P address"},
		"status on p2p":       {func(p *plan) { st := "203.0.113.11:26656"; label(p, "endpoint-1").Status = &st }, "status page uses the node's P2P address"},
		"status low port":     {func(p *plan) { st := "203.0.113.11:80"; label(p, "endpoint-1").Status = &st }, "global unicast"},
		"state sync hash": {func(p *plan) {
			label(p, "sentry-4").StateSync = &stateSync{TrustHeight: 10, TrustHash: "abc"}
		}, "trust hash"},
	} {
		p := fixturePlan(t)
		tc.change(&p)
		if _, err := generateFixture(t, encodePlan(t, p)); err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("%s: %v, want %q", name, err, tc.want)
		}
	}
	// Unknown and missing fields are refused.
	raw := bytes.Replace(encodePlan(t, fixturePlan(t)), []byte(`"chain_id"`), []byte(`"note": 1, "chain_id"`), 1)
	if _, err := generateFixture(t, raw); err == nil || !strings.Contains(err.Error(), "unknown field") {
		t.Errorf("unknown field: %v", err)
	}
	raw = bytes.Replace(encodePlan(t, fixturePlan(t)), []byte(`"channel": null,`), nil, 1)
	if _, err := generateFixture(t, raw); err == nil || !strings.Contains(err.Error(), "missing fields") {
		t.Errorf("missing field: %v", err)
	}
}

// A validator registered after genesis has a key outside the genesis set
// (P01, 3 October 2026); a genesis validator that left the set has no host.
func TestValidatorsMayJoinAfterGenesis(t *testing.T) {
	summary := func(p plan) bindings {
		t.Helper()
		files, err := generateFixture(t, encodePlan(t, p))
		if err != nil {
			t.Fatal(err)
		}
		var out bindings
		if err := strict(filesByPath(files)["PIN_PLAN_BINDINGS.json"], &out); err != nil {
			t.Fatal(err)
		}
		return out
	}
	if got := summary(fixturePlan(t)).GenesisValidatorsWithoutHost; got != 0 {
		t.Fatalf("the launch plan leaves %d genesis validators without a host", got)
	}
	joined := fixturePlan(t)
	joined.Hosts[1].ValidatorPublicKey = syntheticKey("validator", "joined-later")
	if got := summary(joined).GenesisValidatorsWithoutHost; got != 1 {
		t.Fatalf("%d genesis validators without a host", got)
	}
	left := fixturePlan(t)
	left.Hosts = left.Hosts[:3]
	left.Hosts = append(left.Hosts, fixturePlan(t).Hosts[4:]...)
	for i := range left.Hosts {
		if left.Hosts[i].Label == "sentry-4" {
			left.Hosts[i].Pins = left.Hosts[i].Pins[1:]
		}
	}
	if got := summary(left).GenesisValidatorsWithoutHost; got != 1 {
		t.Fatalf("%d genesis validators without a host", got)
	}
}

func TestAStateSyncHostTakesItsTrustedBlock(t *testing.T) {
	p := fixturePlan(t)
	for i := range p.Hosts {
		if p.Hosts[i].Label == "sentry-4" {
			p.Hosts[i].StateSync = &stateSync{TrustHeight: 17280, TrustHash: strings.Repeat("ab", 32)}
		}
	}
	files, err := generateFixture(t, encodePlan(t, p))
	if err != nil {
		t.Fatal(err)
	}
	config := string(filesByPath(files)[filepath.Join("hosts", "sentry-4", "config.toml")])
	for _, want := range []string{"enable = true\n", "trust_height = 17280\n", `trust_hash = "` + strings.Repeat("AB", 32) + `"`, `trust_period = "168h0m0s"`, `rpc_servers = ""`} {
		if !strings.Contains(config, want) {
			t.Fatalf("state sync config lacks %q", want)
		}
	}
}

func TestValuesAreStrict(t *testing.T) {
	values := read(t, filepath.Join(rehearsal, "host-values.json"))
	planRaw := encodePlan(t, fixturePlan(t))
	for name, changed := range map[string][]byte{
		"schema":    bytes.Replace(values, []byte("dytallix.host-values.v1"), []byte("dytallix.host-values.v0"), 1),
		"unknown":   bytes.Replace(values, []byte(`"schema"`), []byte(`"note": 1, "schema"`), 1),
		"duration":  bytes.Replace(values, []byte(`"timeout_commit": "4s"`), []byte(`"timeout_commit": "four"`), 1),
		"redial":    bytes.Replace(values, []byte(`"persistent_peers_max_dial_period": "60s"`), []byte(`"persistent_peers_max_dial_period": "0s"`), 1),
		"tx bytes":  bytes.Replace(values, []byte(`"max_tx_bytes": 262144`), []byte(`"max_tx_bytes": 2097152`), 1),
		"handshake": bytes.Replace(values, []byte(`"handshake_timeout_ms": 5000`), []byte(`"handshake_timeout_ms": 50`), 1),
		"peers":     bytes.Replace(values, []byte(`"max_num_inbound_peers": 64`), []byte(`"max_num_inbound_peers": 65`), 1),
		"missing":   bytes.Replace(values, []byte(`"cache_size": 10000,`), nil, 1),
		"role":      bytes.Replace(values, []byte(`"endpoint": 0,`), []byte(`"observer": 0,`), 1),
	} {
		if bytes.Equal(changed, values) {
			t.Fatalf("%s: no change made", name)
		}
		if _, err := generate(planRaw, changed, read(t, genesisPath), t.TempDir()); err == nil {
			t.Errorf("%s values accepted", name)
		}
	}
}
