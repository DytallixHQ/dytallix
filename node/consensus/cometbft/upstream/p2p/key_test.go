package p2p

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/cometbft/cometbft/crypto"
	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtrand "github.com/cometbft/cometbft/libs/rand"
)

func genPrivKey() crypto.PrivKey {
	privKey, err := mldsa65.GenPrivKey()
	if err != nil {
		panic(err)
	}
	return privKey
}

func TestLoadOrGenNodeKey(t *testing.T) {
	filePath := filepath.Join(os.TempDir(), cmtrand.Str(12)+"_peer_id.json")

	nodeKey, err := LoadOrGenNodeKey(filePath)
	assert.Nil(t, err)

	nodeKey2, err := LoadOrGenNodeKey(filePath)
	assert.Nil(t, err)

	// ML-DSA-65 keys compare by their packed bytes.
	assert.True(t, nodeKey.PrivKey.Equals(nodeKey2.PrivKey))
}

func TestLoadNodeKey(t *testing.T) {
	filePath := filepath.Join(os.TempDir(), cmtrand.Str(12)+"_peer_id.json")

	_, err := LoadNodeKey(filePath)
	assert.True(t, os.IsNotExist(err))

	_, err = LoadOrGenNodeKey(filePath)
	require.NoError(t, err)

	nodeKey, err := LoadNodeKey(filePath)
	assert.NoError(t, err)
	assert.NotNil(t, nodeKey)
}

func TestNodeKeySaveAs(t *testing.T) {
	filePath := filepath.Join(os.TempDir(), cmtrand.Str(12)+"_peer_id.json")

	assert.NoFileExists(t, filePath)

	privKey := genPrivKey()
	nodeKey := &NodeKey{
		PrivKey: privKey,
	}
	err := nodeKey.SaveAs(filePath)
	assert.NoError(t, err)
	assert.FileExists(t, filePath)
}

//----------------------------------------------------------

func padBytes(bz []byte, targetBytes int) []byte {
	return append(bz, bytes.Repeat([]byte{0xFF}, targetBytes-len(bz))...)
}

func TestPoWTarget(t *testing.T) {
	targetBytes := 20
	cases := []struct {
		difficulty uint
		target     []byte
	}{
		{0, padBytes([]byte{}, targetBytes)},
		{1, padBytes([]byte{127}, targetBytes)},
		{8, padBytes([]byte{0}, targetBytes)},
		{9, padBytes([]byte{0, 127}, targetBytes)},
		{10, padBytes([]byte{0, 63}, targetBytes)},
		{16, padBytes([]byte{0, 0}, targetBytes)},
		{17, padBytes([]byte{0, 0, 127}, targetBytes)},
	}

	for _, c := range cases {
		assert.Equal(t, MakePoWTarget(c.difficulty, 20*8), c.target)
	}
}
