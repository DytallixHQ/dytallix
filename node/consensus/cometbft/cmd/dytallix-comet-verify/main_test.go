package main

import "testing"

func TestVerifierOnlyAcceptsNumericLoopbackEndpoints(t *testing.T) {
	for _, good := range []string{"http://127.0.0.1:28651", "http://[::1]:28651/"} {
		if err := loopbackURL(good); err != nil {
			t.Errorf("%s: %v", good, err)
		}
	}
	for _, bad := range []string{"http://example.com:26657", "http://localhost:26657", "http://127.0.0.1", "http://user@127.0.0.1:1", "http://127.0.0.1:1/private", "https://127.0.0.1:1", "http://127.0.0.1:1/?x=1"} {
		if err := loopbackURL(bad); err == nil {
			t.Errorf("accepted %s", bad)
		}
	}
}
