package ownerguard

import (
	"encoding/binary"
	"math"
	"strings"
	"testing"
)

func testFrame() []byte {
	b := make([]byte, 128)
	copy(b, "DYTOWN01")
	binary.BigEndian.PutUint32(b[8:12], uint32(Engine))
	binary.BigEndian.PutUint32(b[12:16], 123)
	binary.BigEndian.PutUint32(b[16:20], 123)
	binary.BigEndian.PutUint32(b[20:24], 62024)
	binary.BigEndian.PutUint64(b[24:32], 1000)
	binary.BigEndian.PutUint64(b[32:40], 5)
	binary.BigEndian.PutUint64(b[40:48], 99)
	b[48] = 1
	return b
}
func TestFrameRejectsMissingMalformedAndMismatchedInputs(t *testing.T) {
	tests := map[string]func([]byte) []byte{
		"missing": func([]byte) []byte { return nil }, "short": func(b []byte) []byte { return b[:127] }, "long": func(b []byte) []byte { return append(b, 0) },
		"magic": func(b []byte) []byte { b[0] = '!'; return b }, "role": func(b []byte) []byte { binary.BigEndian.PutUint32(b[8:12], 5); return b },
		"pid_one": func(b []byte) []byte { binary.BigEndian.PutUint32(b[12:16], 1); return b }, "pid_overflow": func(b []byte) []byte { binary.BigEndian.PutUint32(b[12:16], math.MaxUint32); return b },
		"owner_thread_mismatch": func(b []byte) []byte { binary.BigEndian.PutUint32(b[16:20], 124); return b },
		"expired":               func(b []byte) []byte { binary.BigEndian.PutUint64(b[24:32], 100); return b }, "deadline_overflow": func(b []byte) []byte { binary.BigEndian.PutUint64(b[24:32], math.MaxUint64); return b },
		"zero_inode": func(b []byte) []byte { binary.BigEndian.PutUint64(b[40:48], 0); return b }, "zero_context": func(b []byte) []byte { b[48] = 0; return b }, "reserved": func(b []byte) []byte { b[127] = 1; return b },
	}
	for name, mutate := range tests {
		t.Run(name, func(t *testing.T) {
			if _, e := parseFrame(mutate(testFrame()), Engine, 100); e == nil {
				t.Fatal("invalid frame admitted")
			}
		})
	}
}
func TestFrameBindsEveryField(t *testing.T) {
	f, e := parseFrame(testFrame(), Engine, 100)
	if e != nil {
		t.Fatal(e)
	}
	if f.pid != 123 || f.tid != 123 || f.uid != 62024 || f.deadline != 1000 || f.device != 5 || f.inode != 99 || f.raw[48] != 1 {
		t.Fatal("field lost")
	}
	for _, role := range []Role{0, 6} {
		if _, e := parseFrame(testFrame(), role, 100); e == nil {
			t.Fatal("unknown role")
		}
	}
	if _, e := parseFrame(testFrame(), Engine, -1); e == nil {
		t.Fatal("failed clock")
	}
}
func TestStrictStatusAndParentIdentifier(t *testing.T) {
	if e := validateStatus("Name: guard\nNoNewPrivs:\t1\nSeccomp:\t2\n"); e != nil {
		t.Fatal(e)
	}
	for _, s := range []string{"", "NoNewPrivs: 1\n", "NoNewPrivs: 0\nSeccomp: 2\n", "NoNewPrivs: 1\nSeccomp: 0\n", "NoNewPrivs: 1\nSeccomp: 1\n", "NoNewPrivs: 1\nNoNewPrivs: 1\nSeccomp: 2\n", "NoNewPrivs: 1x\nSeccomp: 2\n"} {
		if e := validateStatus(s); e == nil {
			t.Fatalf("admitted %q", s)
		}
	}
	if v, e := singleDecimal("Pid:\t123\nNSpid:\t123\n", "Pid:"); e != nil || v != 123 {
		t.Fatal("valid fdinfo rejected")
	}
	for _, s := range []string{"Pid: -1\n", "Pid: 123\nPid: 123\n", "Pid: +123\n", "Pid: " + strings.Repeat("9", 40) + "\n"} {
		if _, e := singleDecimal(s, "Pid:"); e == nil {
			t.Fatal("bad pid accepted")
		}
	}
}
