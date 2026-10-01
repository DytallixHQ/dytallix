//go:build production

package enginepqc

// ProductionBuild is true in a build with the production tag. It has no
// development or staging transport profile; the production transport profile
// is production activation step A4, so this build starts no chain yet.
const ProductionBuild = true
