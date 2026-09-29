package encoding

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	pc "github.com/cometbft/cometbft/proto/tendermint/crypto"
)

func genPubKey(t *testing.T) crypto.PubKey {
	t.Helper()
	privKey, err := mldsa65.GenPrivKey()
	require.NoError(t, err)
	return privKey.PubKey()
}

func TestPubKeyToFromProto(t *testing.T) {
	pk := genPubKey(t)
	proto, err := PubKeyToProto(pk)
	require.NoError(t, err)

	pubkey, err := PubKeyFromProto(proto)
	require.NoError(t, err)
	assert.Equal(t, pk.Type(), pubkey.Type())
	assert.Equal(t, pk.Bytes(), pubkey.Bytes())
	assert.Equal(t, pk.Address(), pubkey.Address())
	assert.Equal(t, pk.VerifySignature([]byte("msg"), []byte("sig")), pubkey.VerifySignature([]byte("msg"), []byte("sig")))

	// Only ML-DSA-65 keys are encoded or decoded.
	_, err = PubKeyToProto(nil)
	assert.Error(t, err)
	_, err = PubKeyFromProto(pc.PublicKey{})
	assert.Error(t, err)
}

func TestPubKeyFromTypeAndBytes(t *testing.T) {
	pk := genPubKey(t)
	pubkey, err := PubKeyFromTypeAndBytes(pk.Type(), pk.Bytes())
	assert.NoError(t, err)
	assert.Equal(t, pk.Type(), pubkey.Type())
	assert.Equal(t, pk.Bytes(), pubkey.Bytes())
	assert.Equal(t, pk.Address(), pubkey.Address())
	assert.Equal(t, pk.VerifySignature([]byte("msg"), []byte("sig")), pubkey.VerifySignature([]byte("msg"), []byte("sig")))

	// invalid size
	_, err = PubKeyFromTypeAndBytes(pk.Type(), pk.Bytes()[:10])
	assert.Error(t, err)

	// other key types
	for _, keyType := range []string{"", "ed25519", "secp256k1", "secp256k1eth", "bls12381"} {
		_, err = PubKeyFromTypeAndBytes(keyType, pk.Bytes())
		assert.Error(t, err, keyType)
	}
}
