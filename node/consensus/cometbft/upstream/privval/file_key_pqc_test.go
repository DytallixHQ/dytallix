//go:build dytallix_pqc_only

package privval

import (
	"encoding/base64"
	"fmt"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/cometbft/cometbft/crypto/mldsa65"
	cmtjson "github.com/cometbft/cometbft/libs/json"
)

func TestUnmarshalValidatorKeyMLDSA65(t *testing.T) {
	assert, require := assert.New(t), require.New(t)

	// create some fixed values
	privKey, err := mldsa65.GenPrivKey()
	require.Nil(err)
	pubKey := privKey.PubKey()
	addr := pubKey.Address()
	pubBytes := pubKey.Bytes()
	privBytes := privKey.Bytes()
	pubB64 := base64.StdEncoding.EncodeToString(pubBytes)
	privB64 := base64.StdEncoding.EncodeToString(privBytes)

	serialized := fmt.Sprintf(`{
  "address": "%s",
  "pub_key": {
    "type": "%s",
    "value": "%s"
  },
  "priv_key": {
    "type": "%s",
    "value": "%s"
  }
}`, addr, mldsa65.PubKeyName, pubB64, mldsa65.PrivKeyName, privB64)

	val := FilePVKey{}
	err = cmtjson.Unmarshal([]byte(serialized), &val)
	require.Nil(err, "%+v", err)

	// make sure the values match
	assert.EqualValues(addr, val.Address)
	// Keys wrap internal state; compare their canonical encodings.
	assert.Equal(pubKey.Bytes(), val.PubKey.Bytes())
	assert.Equal(privKey.Bytes(), val.PrivKey.Bytes())

	// export it and make sure it is the same
	out, err := cmtjson.Marshal(val)
	require.Nil(err, "%+v", err)
	assert.JSONEq(serialized, string(out))
}
