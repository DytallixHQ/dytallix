package enginepqc

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"

	"dytallix.local/consensus/cometbft/internal/pqcp2p"
)

// PSKFileName holds this host's pre-shared keys, one per pinned peer, for wire
// version 3. It is a secret and belongs in the sealed host bundle, never in the
// public pqc_transport.json. Both hosts of a pair hold the same key.
const PSKFileName = "pqc_peer_psk.json"

type pskFile struct {
	Version int               `json:"version"`
	Keys    map[string]string `json:"keys"` // canonical base64 full pin -> base64 32-byte key
}

// loadPeerPSKs reads the optional PSK file. Without one, every pair uses the
// zero PSK and the transport is only as strong as ML-KEM and ML-DSA. A
// production profile should require the file; this spike does not yet.
func loadPeerPSKs(home string, pins [][]byte) (map[string]pqcp2p.PSK, error) {
	path := filepath.Join(home, "config", PSKFileName)
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return map[string]pqcp2p.PSK{}, nil
	}
	raw, err := privateFile(path, 1<<16)
	if err != nil {
		return nil, err
	}
	defer clear(raw)
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var file pskFile
	if err = decoder.Decode(&file); err != nil || decoder.More() || file.Version != 1 || len(file.Keys) == 0 || len(file.Keys) > pqcp2p.MaxPins {
		return nil, errors.New("peer PSK file must be version 1 with one key per pinned peer")
	}
	pinned := make(map[string]bool, len(pins))
	for _, pin := range pins {
		pinned[string(pin)] = true
	}
	keys := make(map[string]pqcp2p.PSK, len(file.Keys))
	for encodedPin, encodedKey := range file.Keys {
		pin, err := decodePin(encodedPin)
		if err != nil || !pinned[string(pin)] {
			return nil, errors.New("peer PSK names a peer that is not pinned")
		}
		key, err := base64.StdEncoding.Strict().DecodeString(encodedKey)
		if err != nil || len(key) != len(pqcp2p.PSK{}) || bytes.Equal(key, make([]byte, len(key))) {
			clear(key)
			return nil, errors.New("peer PSK must be 32 nonzero bytes")
		}
		var psk pqcp2p.PSK
		copy(psk[:], key)
		clear(key)
		keys[string(pin)] = psk
	}
	return keys, nil
}

// peerPSK returns the pair's key, or the zero PSK for a peer without one.
func (r *Runtime) peerPSK(pin []byte) pqcp2p.PSK {
	return r.psk[string(pin)]
}
