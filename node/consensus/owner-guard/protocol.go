// Package ownerguard admits one inherited owner channel before executable work.
// It does not qualify a release, AppArmor policy, production deployment or re-exec.
package ownerguard

import (
	"bytes"
	"encoding/binary"
	"errors"
	"math"
	"strconv"
	"strings"
)

type Role uint32

const (
	Application Role = 1
	Bridge      Role = 2
	Engine      Role = 3
	Adapter     Role = 4
	Helper      Role = 5
	controlFD        = 7
	parentFD         = 8
	frameSize        = 128
)

var errAdmission = errors.New("owner startup admission rejected")

type frame struct {
	raw           [frameSize]byte
	role          Role
	pid, tid, uid uint32
	deadline      int64
	device, inode uint64
}

func parseFrame(raw []byte, role Role, now int64) (frame, error) {
	var f frame
	if len(raw) != frameSize || role < Application || role > Helper || !bytes.Equal(raw[:8], []byte("DYTOWN01")) {
		return f, errAdmission
	}
	f.role = Role(binary.BigEndian.Uint32(raw[8:12]))
	f.pid = binary.BigEndian.Uint32(raw[12:16])
	f.tid = binary.BigEndian.Uint32(raw[16:20])
	f.uid = binary.BigEndian.Uint32(raw[20:24])
	deadline := binary.BigEndian.Uint64(raw[24:32])
	f.device = binary.BigEndian.Uint64(raw[32:40])
	f.inode = binary.BigEndian.Uint64(raw[40:48])
	if f.role != role || f.pid <= 1 || f.pid > math.MaxInt32 || f.tid != f.pid || deadline > math.MaxInt64 || now < 0 || deadline <= uint64(now) || f.inode == 0 {
		return frame{}, errAdmission
	}
	for _, v := range raw[112:] {
		if v != 0 {
			return frame{}, errAdmission
		}
	}
	if bytes.Equal(raw[48:112], make([]byte, 64)) {
		return frame{}, errAdmission
	}
	f.deadline = int64(deadline)
	copy(f.raw[:], raw)
	return f, nil
}
func singleDecimal(text, key string) (uint64, error) {
	var found bool
	var value uint64
	for _, line := range strings.Split(text, "\n") {
		if !strings.HasPrefix(line, key) {
			continue
		}
		if found {
			return 0, errAdmission
		}
		found = true
		raw := strings.TrimLeft(line[len(key):], " \t")
		if raw == "" {
			return 0, errAdmission
		}
		for _, b := range []byte(raw) {
			if b < '0' || b > '9' {
				return 0, errAdmission
			}
		}
		n, e := strconv.ParseUint(raw, 10, 64)
		if e != nil {
			return 0, errAdmission
		}
		value = n
	}
	if !found {
		return 0, errAdmission
	}
	return value, nil
}
func validateStatus(text string) error {
	n, e := singleDecimal(text, "NoNewPrivs:")
	if e != nil || n != 1 {
		return errAdmission
	}
	n, e = singleDecimal(text, "Seccomp:")
	if e != nil || n != 2 {
		return errAdmission
	}
	return nil
}
