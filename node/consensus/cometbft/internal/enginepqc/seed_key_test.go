package enginepqc

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/cometbft/cometbft/crypto/mldsa65"
)

func seedHome(t *testing.T) (string, string, []byte) {
	t.Helper()
	home := t.TempDir()
	config := filepath.Join(home, "config")
	if err := os.Mkdir(config, 0o700); err != nil {
		t.Fatal(err)
	}
	seed := bytes.Repeat([]byte{0x36}, mldsa65.SeedSize)
	private, err := mldsa65.GenPrivKeyFromSeed(seed)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(config, SeedFileName)
	if err := os.WriteFile(path, seed, 0o600); err != nil {
		t.Fatal(err)
	}
	return home, path, private.PubKey().Bytes()
}

func TestSeedNodeKeyMatchesFullPinAndTransportIdentity(t *testing.T) {
	home, _, pin := seedHome(t)
	key, identity, err := seedNodeKey(home, pin)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(key.PubKey().Bytes(), identity.PublicKey()) {
		t.Fatal("transport and Comet peer keys differ")
	}
	other, _, err := seedNodeKey(home, pin)
	if err != nil || other.ID() != key.ID() {
		t.Fatalf("restart changed peer identity: %v", err)
	}
	bad := bytes.Clone(pin)
	bad[0] ^= 1
	if _, _, err := seedNodeKey(home, bad); err == nil {
		t.Fatal("wrong full public pin accepted")
	}
}

func TestSeedNodeKeyRejectsUnsafeInput(t *testing.T) {
	cases := map[string]func(*testing.T, string, string){
		"missing": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.Remove(path); err != nil {
				t.Fatal(err)
			}
		},
		"short": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.WriteFile(path, []byte{1}, 0o600); err != nil {
				t.Fatal(err)
			}
		},
		"long": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.WriteFile(path, bytes.Repeat([]byte{1}, mldsa65.SeedSize+1), 0o600); err != nil {
				t.Fatal(err)
			}
		},
		"public mode": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.Chmod(path, 0o644); err != nil {
				t.Fatal(err)
			}
		},
		"executable mode": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.Chmod(path, 0o700); err != nil {
				t.Fatal(err)
			}
		},
		"symlink": func(t *testing.T, _, path string) {
			t.Helper()
			target := path + ".target"
			if err := os.Rename(path, target); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(target, path); err != nil {
				t.Fatal(err)
			}
		},
		"hardlink": func(t *testing.T, _, path string) {
			t.Helper()
			if err := os.Link(path, path+".link"); err != nil {
				t.Fatal(err)
			}
		},
		"packed key present": func(t *testing.T, home, _ string) {
			t.Helper()
			if err := os.WriteFile(filepath.Join(home, "config", "node_key.json"), []byte("{}"), 0o600); err != nil {
				t.Fatal(err)
			}
		},
		"packed key symlink": func(t *testing.T, home, path string) {
			t.Helper()
			if err := os.Symlink(path, filepath.Join(home, "config", "node_key.json")); err != nil {
				t.Fatal(err)
			}
		},
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			home, path, pin := seedHome(t)
			change(t, home, path)
			if _, _, err := seedNodeKey(home, pin); err == nil {
				t.Fatal("unsafe seed input accepted")
			}
		})
	}
}
