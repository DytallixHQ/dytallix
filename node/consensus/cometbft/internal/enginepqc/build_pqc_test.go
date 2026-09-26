//go:build dytallix_pqc_only

package enginepqc

import (
	"bytes"
	"net"
	"path/filepath"
	"strings"
	"testing"
	"time"

	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/crypto/batch"
	encoding "github.com/cometbft/cometbft/crypto/encoding"
	"github.com/cometbft/cometbft/crypto/keydefaults"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	"github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/p2p/conn"
	"github.com/cometbft/cometbft/privval"
	proto "github.com/cometbft/cometbft/proto/tendermint/crypto"
	"github.com/cometbft/cometbft/types"
)

func TestPQCBuildCodecRoundTrip(t *testing.T) {
	key, err := keydefaults.Generate()
	if err != nil {
		t.Fatal(err)
	}
	if key.Type() != mldsa65.KeyType || BuildProfile != "dytallix_pqc_only" {
		t.Fatal("wrong profile default")
	}
	message := []byte("PQC build profile roundtrip")
	signature, err := key.Sign(message)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := encoding.PubKeyToProto(key.PubKey())
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := encoding.PubKeyFromProto(wire)
	if err != nil {
		t.Fatal(err)
	}
	if !decoded.VerifySignature(message, signature) || !bytes.Equal(decoded.Bytes(), key.PubKey().Bytes()) {
		t.Fatal("ML-DSA roundtrip failed")
	}
	if verifier, supported := batch.CreateBatchVerifier(decoded); verifier != nil || supported || batch.SupportsBatchVerifier(decoded) {
		t.Fatal("unexpected batch implementation")
	}
}

func TestPQCBuildCodecRejectsClassicalWireTypes(t *testing.T) {
	cases := []proto.PublicKey{
		{Sum: &proto.PublicKey_Ed25519{Ed25519: make([]byte, 32)}},
		{Sum: &proto.PublicKey_Secp256K1{Secp256K1: make([]byte, 33)}},
		{Sum: &proto.PublicKey_Bls12381{Bls12381: make([]byte, 48)}},
		{Sum: &proto.PublicKey_Secp256K1Eth{Secp256K1Eth: make([]byte, 33)}},
		{}, {Sum: (*proto.PublicKey_Mldsa65)(nil)},
	}
	for i, wire := range cases {
		if _, err := encoding.PubKeyFromProto(wire); err == nil {
			t.Fatalf("wire type %d accepted", i)
		}
	}
	for _, kind := range []string{"", "ed25519", "secp256k1", "bls12381", "secp256k1eth"} {
		if _, err := encoding.PubKeyFromTypeAndBytes(kind, make([]byte, 32)); err == nil {
			t.Fatalf("type %q accepted", kind)
		}
	}
	if _, err := encoding.PubKeyFromTypeAndBytes(mldsa65.KeyType, make([]byte, 32)); err == nil {
		t.Fatal("invalid ML-DSA length accepted")
	}
	var node p2p.NodeKey
	if err := cmtjson.Unmarshal([]byte(`{"priv_key":{"type":"tendermint/PrivKeyEd25519","value":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="}}`), &node); err == nil {
		t.Fatal("classical JSON key accepted")
	}
}

func TestPQCBuildConsensusRejectsClassicalParameters(t *testing.T) {
	params := types.DefaultConsensusParams()
	if err := params.ValidateBasic(); err != nil {
		t.Fatal(err)
	}
	if len(params.Validator.PubKeyTypes) != 1 || params.Validator.PubKeyTypes[0] != mldsa65.KeyType {
		t.Fatal("wrong default validator key")
	}
	for _, kind := range []string{"ed25519", "secp256k1", "bls12381", "secp256k1eth"} {
		params.Validator.PubKeyTypes = []string{kind}
		if params.ValidateBasic() == nil {
			t.Fatalf("classical consensus type %s accepted", kind)
		}
	}
}

func TestPQCBuildNodeKeyGeneration(t *testing.T) {
	path := filepath.Join(t.TempDir(), "node_key.json")
	key, err := p2p.LoadOrGenNodeKey(path)
	if err != nil {
		t.Fatal(err)
	}
	loaded, err := p2p.LoadNodeKey(path)
	if err != nil {
		t.Fatal(err)
	}
	if key.PrivKey.Type() != mldsa65.KeyType || loaded.ID() != key.ID() {
		t.Fatal("generated node key was not ML-DSA")
	}
}

func TestPQCBuildListenerRequiresExplicitUpgrade(t *testing.T) {
	key := testKey(t)
	transport := p2p.NewMultiplexTransport(p2p.DefaultNodeInfo{}, *key, conn.DefaultMConnConfig())
	defer transport.Close()
	addr := p2p.NewNetAddressIPPort(net.ParseIP("127.0.0.1"), 0)
	if err := transport.Listen(*addr); err == nil || !strings.Contains(err.Error(), "PQC upgrade required") {
		t.Fatalf("listener did not fail closed: %v", err)
	}
}

func TestPQCBuildRemoteSignerRejected(t *testing.T) {
	key := testKey(t)
	for _, address := range []string{"tcp://127.0.0.1:1", "unix:///unopened.sock", "noise://unused"} {
		if listener, err := privval.NewSignerListenerFromAddr(address, key.PrivKey, log.NewNopLogger()); listener != nil || err == nil {
			t.Fatal("remote signer listener accepted")
		}
	}
	if connection, err := privval.DialTCPFn("tcp://127.0.0.1:1", time.Second, key.PrivKey)(); connection != nil || err == nil || !strings.Contains(err.Error(), "excluded") {
		t.Fatal("legacy signer dial accepted")
	}
}

func TestPQCBuildValidatorUpdateRejectsClassical(t *testing.T) {
	key := testKey(t)
	update := abci.UpdateValidator(key.PubKey().Bytes(), 7, mldsa65.KeyType)
	if update.Power != 7 {
		t.Fatal("wrong update")
	}
	defer func() {
		if recover() == nil {
			t.Fatal("classical update did not fail closed")
		}
	}()
	abci.Ed25519ValidatorUpdate(make([]byte, 32), 7)
}
