//go:build !linux

package ownerguard

import "errors"

// Run fails on unsupported platforms. There is no unguarded production fallback.
func Run(role Role, work func() error) error {
	return errors.New("owner startup admission requires Linux")
}
