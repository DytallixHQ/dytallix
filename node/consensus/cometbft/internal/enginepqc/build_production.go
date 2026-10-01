//go:build production

package enginepqc

// ProductionBuild is true in a build with the production tag. It runs only
// the production transport profile (production activation v1, A4) and has no
// development or staging profile.
const ProductionBuild = true
