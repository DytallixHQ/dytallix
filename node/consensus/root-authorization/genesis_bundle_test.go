package rootauthorization

import "encoding/binary"

func genesisBundleForTest(app, config []byte, engine, release [64]byte) []byte {
	b := []byte("DYTALLIX/ROOT-GENESIS/v1\x00")
	b = binary.BigEndian.AppendUint64(b, uint64(len(app)))
	b = append(b, app...)
	b = binary.BigEndian.AppendUint64(b, uint64(len(config)))
	b = append(b, config...)
	b = append(b, engine[:]...)
	b = append(b, release[:]...)
	return b
}
