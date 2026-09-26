//go:build !linux

package main

import "errors"

func inheritedApplication(input, output int) (*child, error) {
	return nil, errors.New("inherited-pipes-v1 requires Linux")
}
