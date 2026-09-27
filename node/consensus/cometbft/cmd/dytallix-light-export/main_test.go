package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestExportNeedsArgumentsAndANewOutputDirectory(t *testing.T) {
	existing := t.TempDir()
	for _, args := range [][]string{
		nil,
		{"--home", "/h", "--from", "1", "--to", "3"},
		{"--home", "/h", "--from", "1", "--to", "3", "--output", "relative"},
		{"--home", "/h", "--from", "1", "--to", "3", "--output", existing},
		{"--home", "/h", "--output", filepath.Join(t.TempDir(), "new"), "extra"},
	} {
		if err := run(args); err == nil {
			t.Fatal("accepted", args)
		}
	}
	// A new directory is made before the home is read.
	output := filepath.Join(t.TempDir(), "new")
	if err := run([]string{"--home", "/missing", "--from", "1", "--to", "3", "--output", output}); err == nil {
		t.Fatal("exported from a missing home")
	}
	if _, err := os.Stat(output); err != nil {
		t.Fatal(err)
	}
}
