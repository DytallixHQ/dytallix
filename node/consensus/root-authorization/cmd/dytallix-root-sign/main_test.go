package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func runOK(t *testing.T, args ...string) string {
	t.Helper()
	var out bytes.Buffer
	if err := run(args, &out); err != nil {
		t.Fatalf("%v: %v", args, err)
	}
	return out.String()
}

func TestOfflineSignerFlow(t *testing.T) {
	dir := t.TempDir()
	path := func(name string) string { return filepath.Join(dir, name) }
	var keys []string
	for i := 0; i < 5; i++ {
		runOK(t, "keygen", "-private-key-out", path(fmt.Sprintf("key%d.private", i)), "-public-key-out", path(fmt.Sprintf("key%d.json", i)))
		keys = append(keys, path(fmt.Sprintf("key%d.json", i)))
		info, err := os.Stat(path(fmt.Sprintf("key%d.private", i)))
		if err != nil || info.Mode().Perm() != 0o600 || info.Size() != 128 {
			t.Fatal("private key file must be 128 bytes, mode 0600", err)
		}
	}
	if err := run(append([]string{"policy", "-chain-id", "root-sign-fixture", "-out", path("policy.json")}, keys[:4]...), &bytes.Buffer{}); err == nil {
		t.Fatal("a four-key policy accepted")
	}
	runOK(t, append([]string{"policy", "-chain-id", "root-sign-fixture", "-out", path("policy.json")}, keys...)...)

	for name, content := range map[string]string{"app.json": `{"app":1}`, "config.json": `{"config":1}`, "engine.json": `{"engine":1}`, "release.json": `{"release":1}`} {
		if err := os.WriteFile(path(name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	var digests map[string]string
	if err := json.Unmarshal([]byte(runOK(t, "digest", "-native-genesis", path("app.json"), "-config", path("config.json"),
		"-engine-genesis", path("engine.json"), "-release-manifest", path("release.json"))), &digests); err != nil {
		t.Fatal(err)
	}
	bundle := digests["bundle_sha512"]
	if len(bundle) != 128 {
		t.Fatal("bundle digest missing")
	}

	if err := os.Chmod(path("key0.private"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"sign", "-policy", path("policy.json"), "-private-key", path("key0.private"), "-bundle-sha512", bundle, "-out", path("sig0.json")}, &bytes.Buffer{}); err == nil {
		t.Fatal("a group-readable private key accepted")
	}
	if err := os.Chmod(path("key0.private"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run([]string{"sign", "-policy", path("policy.json"), "-private-key", path("key0.private"), "-bundle-sha512", strings.ToUpper(bundle), "-out", path("sig0.json")}, &bytes.Buffer{}); err == nil {
		t.Fatal("a noncanonical bundle digest accepted")
	}
	var parts []string
	for i := 0; i < 3; i++ {
		out := path(fmt.Sprintf("sig%d.json", i))
		runOK(t, "sign", "-policy", path("policy.json"), "-private-key", path(fmt.Sprintf("key%d.private", i)), "-bundle-sha512", bundle, "-out", out)
		parts = append(parts, out)
	}
	if err := run([]string{"sign", "-policy", path("policy.json"), "-private-key", path("key0.private"), "-bundle-sha512", bundle, "-out", parts[0]}, &bytes.Buffer{}); err == nil {
		t.Fatal("an existing output was replaced")
	}
	if err := run(append([]string{"combine", "-policy", path("policy.json"), "-out", path("two.json")}, parts[:2]...), &bytes.Buffer{}); err == nil {
		t.Fatal("two of five combined")
	}
	if !strings.Contains(runOK(t, append([]string{"combine", "-policy", path("policy.json"), "-out", path("signatures.json")}, parts...)...), "3 of 5") {
		t.Fatal("combine did not report three of five")
	}
	runOK(t, "verify", "-policy", path("policy.json"), "-signatures", path("signatures.json"), "-bundle-sha512", bundle)
	other := []byte(bundle)
	other[0] = map[bool]byte{true: '1', false: '0'}[other[0] == '0']
	if err := run([]string{"verify", "-policy", path("policy.json"), "-signatures", path("signatures.json"), "-bundle-sha512", string(other)}, &bytes.Buffer{}); err == nil {
		t.Fatal("signatures verified over another bundle")
	}
}

func TestOfflineSignerRefusesIncompleteArguments(t *testing.T) {
	for _, args := range [][]string{{}, {"unknown"}, {"keygen"}, {"sign", "-policy", "p"}, {"combine", "-policy", "p", "-out", "o"}} {
		if err := run(args, &bytes.Buffer{}); err == nil {
			t.Fatalf("%v accepted", args)
		}
	}
}
