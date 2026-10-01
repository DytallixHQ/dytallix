package rootauthorization

import (
	"crypto/sha512"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sort"
	"testing"
)

type genesisSigner struct {
	public, private []byte
}

func genesisFixture(t *testing.T) (GenesisPolicy, []genesisSigner) {
	t.Helper()
	signers := make([]genesisSigner, GenesisSignerCount)
	policy := GenesisPolicy{Schema: GenesisRecordSchema, ChainID: "root-threshold-fixture", Authority: Authority{Threshold: GenesisThreshold}}
	for i := range signers {
		public, private := localPair(t)
		signers[i] = genesisSigner{public, private}
		policy.Authority.Keys = append(policy.Authority.Keys, AuthorityKey{KeyID: KeyID(public), PublicKeyHex: hex.EncodeToString(public)})
	}
	sort.Slice(policy.Authority.Keys, func(i, j int) bool { return policy.Authority.Keys[i].KeyID < policy.Authority.Keys[j].KeyID })
	if err := policy.Validate(); err != nil {
		t.Fatal(err)
	}
	return policy, signers
}

func TestGenesisPolicyShape(t *testing.T) {
	policy, signers := genesisFixture(t)
	for name, change := range map[string]func(*GenesisPolicy){
		"schema":      func(p *GenesisPolicy) { p.Schema = 2 },
		"chain":       func(p *GenesisPolicy) { p.ChainID = "bad chain" },
		"two of five": func(p *GenesisPolicy) { p.Authority.Threshold = 2 },
		"four keys":   func(p *GenesisPolicy) { p.Authority.Keys = p.Authority.Keys[:4] },
		"unsorted": func(p *GenesisPolicy) {
			p.Authority.Keys[0], p.Authority.Keys[1] = p.Authority.Keys[1], p.Authority.Keys[0]
		},
		"duplicate":       func(p *GenesisPolicy) { p.Authority.Keys[1] = p.Authority.Keys[0] },
		"wrong key id":    func(p *GenesisPolicy) { p.Authority.Keys[0].KeyID = KeyID(signers[0].private[:64]) },
		"uppercase hex":   func(p *GenesisPolicy) { p.Authority.Keys[0].PublicKeyHex = "AB" + p.Authority.Keys[0].PublicKeyHex[2:] },
		"short key bytes": func(p *GenesisPolicy) { p.Authority.Keys[0].PublicKeyHex = p.Authority.Keys[0].PublicKeyHex[:126] },
	} {
		bad := policy
		bad.Authority.Keys = append([]AuthorityKey(nil), policy.Authority.Keys...)
		change(&bad)
		if bad.Validate() == nil {
			t.Fatalf("%s accepted", name)
		}
	}
	// The public records are canonical JSON in the node's field order.
	raw, err := json.Marshal(policy)
	if err != nil {
		t.Fatal(err)
	}
	var decoded GenesisPolicy
	if err := DecodeCanonical(raw, &decoded); err != nil || decoded.Validate() != nil {
		t.Fatal("policy does not round-trip canonically", err)
	}
	if string(raw[:40]) != `{"schema":1,"chain_id":"root-threshold-f` {
		t.Fatalf("field order differs: %s", raw[:40])
	}
}

func TestGenesisThreeOfFive(t *testing.T) {
	policy, signers := genesisFixture(t)
	bundle := sha512.Sum512(GenesisBundle([]byte("app"), []byte("config"), sha512.Sum512([]byte("engine")), sha512.Sum512([]byte("release"))))
	var parts []GenesisSignatures
	for _, signer := range signers[:3] {
		part, err := SignGenesis(policy, signer.private, bundle)
		if err != nil {
			t.Fatal(err)
		}
		if len(part.Signatures) != 1 || part.Signatures[0].KeyID != KeyID(signer.public) {
			t.Fatal("one signature by the signer's key expected")
		}
		parts = append(parts, part)
	}
	if _, err := CombineGenesis(policy, parts[:2]); err == nil {
		t.Fatal("two of five accepted")
	}
	combined, err := CombineGenesis(policy, parts)
	if err != nil {
		t.Fatal(err)
	}
	if !sort.SliceIsSorted(combined.Signatures, func(i, j int) bool { return combined.Signatures[i].KeyID < combined.Signatures[j].KeyID }) {
		t.Fatal("combined signatures are not sorted by key ID")
	}
	if VerifyGenesis(policy, combined, bundle) != nil {
		t.Fatal("combined file does not verify")
	}
	other := bundle
	other[0] ^= 1
	if VerifyGenesis(policy, combined, other) == nil {
		t.Fatal("signatures verify over another bundle")
	}
	if _, err := CombineGenesis(policy, append(parts, parts[0])); err == nil {
		t.Fatal("a repeated signer accepted")
	}
	changed := combined
	changed.Signatures = append([]GenesisSignature(nil), combined.Signatures...)
	raw, _ := hex.DecodeString(changed.Signatures[1].SignatureHex)
	raw[100] ^= 1
	changed.Signatures[1].SignatureHex = hex.EncodeToString(raw)
	if err := VerifyGenesis(policy, changed, bundle); !errors.Is(err, ErrSignature) {
		t.Fatalf("a changed signature: %v", err)
	}
	// A key outside the policy cannot sign for it.
	_, outsider := localPair(t)
	if _, err := SignGenesis(policy, outsider, bundle); !errors.Is(err, ErrPolicy) {
		t.Fatalf("an outside key signed: %v", err)
	}
	// Another chain's policy with the same keys does not accept these.
	moved := policy
	moved.ChainID = "another-chain"
	if VerifyGenesis(moved, combined, bundle) == nil {
		t.Fatal("signatures moved to another chain")
	}
}
