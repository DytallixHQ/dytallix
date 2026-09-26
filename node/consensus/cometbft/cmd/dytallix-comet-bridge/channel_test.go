package main

import (
	"context"
	"flag"
	"testing"
)

func TestChannelFlagsRejectAmbiguousModesWithoutSpawn(t *testing.T) {
	set := func(v string) singleFlag { return singleFlag{value: v, set: true} }
	for _, tc := range []struct {
		mode, input, output singleFlag
		args                []string
	}{
		{input: set("3")},
		{mode: set("")},
		{mode: set("auto"), input: set("3"), output: set("4")},
		{mode: set("inherited-pipes-v1"), input: set("3")},
		{mode: set("inherited-pipes-v1"), input: set("3"), output: set("4"), args: []string{"must-never-spawn"}},
	} {
		if _, err := selectApplication(context.Background(), tc.mode, tc.input, tc.output, tc.args); err == nil {
			t.Fatal("accepted ambiguous selection")
		}
	}
	for _, value := range []string{"", "0", "2", "-1", "+3", "03", " 3", "3.0", "9999999999999999999999"} {
		if _, err := channelFD(value); err == nil {
			t.Fatalf("accepted descriptor %q", value)
		}
	}
	if n, err := channelFD("3"); err != nil || n != 3 {
		t.Fatal(n, err)
	}
}

func TestChannelFlagsRejectRepeats(t *testing.T) {
	for _, name := range []string{"application-channel", "application-input-fd", "application-output-fd"} {
		var value singleFlag
		flags := flag.NewFlagSet("test", flag.ContinueOnError)
		flags.Var(&value, name, "")
		if flags.Parse([]string{"--" + name + "=3", "--" + name + "=4"}) == nil {
			t.Fatal("accepted repeated flag", name)
		}
	}
}
