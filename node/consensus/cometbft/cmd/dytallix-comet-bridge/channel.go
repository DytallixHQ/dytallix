package main

import (
	"context"
	"errors"
	"strconv"
)

// Each channel flag is single-use. In particular, a later value cannot mask
// an earlier mode or descriptor selection.
type singleFlag struct {
	value string
	set   bool
}

func (v *singleFlag) String() string { return v.value }
func (v *singleFlag) Set(raw string) error {
	if v.set {
		return errors.New("application channel flag repeated")
	}
	v.value, v.set = raw, true
	return nil
}

func channelFD(raw string) (int, error) {
	n, err := strconv.Atoi(raw)
	if err != nil || n < 3 || strconv.Itoa(n) != raw {
		return 0, errors.New("application descriptor must be a canonical integer >= 3")
	}
	return n, nil
}

func selectApplication(ctx context.Context, mode, input, output singleFlag, args []string) (*child, error) {
	if !mode.set {
		if input.set || output.set {
			return nil, errors.New("application descriptors require explicit inherited-pipes-v1 mode")
		}
		return startChild(ctx, args)
	}
	if mode.value != "inherited-pipes-v1" || !input.set || !output.set || len(args) != 0 {
		return nil, errors.New("inherited-pipes-v1 requires exactly two descriptors and no application command")
	}
	in, err := channelFD(input.value)
	if err != nil {
		return nil, err
	}
	out, err := channelFD(output.value)
	if err != nil {
		return nil, err
	}
	return inheritedApplication(in, out)
}

// The native launch owner owns the application process. The bridge owns only
// these adopted endpoints. It never kills a process in inherited mode.
func (c *child) closeInherited() {
	c.closeOnce.Do(func() {
		_ = c.input.Close()
		_ = c.output.Close()
	})
}
