//go:build !production

package enginepqc

// ProductionBuild is false in a development build, which runs only the
// development and staging transport profiles (production activation v1, A1).
const ProductionBuild = false
