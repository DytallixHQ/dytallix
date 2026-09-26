//go:build !linux

package main

import "testing"

func TestInheritedChannelRequiresLinux(t *testing.T) {
	if _, err := inheritedApplication(3, 4); err == nil {
		t.Fatal("accepted inherited channel on unsupported platform")
	}
}
