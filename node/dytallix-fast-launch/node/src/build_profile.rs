//! The profiles this build runs (production activation v1, step A1).
//!
//! A development build runs only the local-qualification and development
//! profiles; a build with the `production` feature runs only the production
//! profiles. Each refuses the other's, so neither can open the other's chain.
//! The names below become permanent at genesis: the node stores the exact
//! configuration and compares it on every start.
//!
//! A production build opens no chain until the root-signed production path
//! exists (step A2), and refuses root controls until step A4.
use anyhow::{ensure, Result};

/// True in a build with the `production` feature.
pub const PRODUCTION: bool = cfg!(feature = "production");

#[cfg(not(feature = "production"))]
mod names {
    pub const FIXED: Option<&str> = Some("cometbft-local-qualification");
    pub const LIFECYCLE_ONLY: Option<&str> = Some("cometbft-lifecycle-local-qualification");
    pub const PENALTY: &str = "cometbft-penalty-local-qualification";
    pub const LIFECYCLE: &str = "cometbft-lifecycle-local-qualification";
    pub const MONETARY: &str = "development";
    pub const PROFILE_REFUSAL: &str = "Production or unsupported engine profile is disabled";
}

#[cfg(feature = "production")]
mod names {
    pub const FIXED: Option<&str> = None;
    pub const LIFECYCLE_ONLY: Option<&str> = None;
    pub const PENALTY: &str = "cometbft-production-v1";
    pub const LIFECYCLE: &str = "cometbft-lifecycle-production-v1";
    pub const MONETARY: &str = "production";
    pub const PROFILE_REFUSAL: &str =
        "A production build refuses development and unsupported engine profiles";
}

/// The consensus profile without lifecycle (development builds only).
pub const FIXED_PROFILE: Option<&str> = names::FIXED;
/// The consensus profile with lifecycle and no penalties (development builds only).
pub const LIFECYCLE_ONLY_PROFILE: Option<&str> = names::LIFECYCLE_ONLY;
/// The consensus profile with lifecycle and penalties; the penalty
/// configuration carries the same name.
pub const PENALTY_PROFILE: &str = names::PENALTY;
/// The lifecycle configuration's profile.
pub const LIFECYCLE_PROFILE: &str = names::LIFECYCLE;
/// The reward (v2) and issuance profile in the native genesis.
pub const MONETARY_PROFILE: &str = names::MONETARY;
/// The error for a consensus profile this build does not run.
pub const PROFILE_REFUSAL: &str = names::PROFILE_REFUSAL;

/// The consensus profiles this build runs.
pub fn consensus_profiles() -> impl Iterator<Item = &'static str> {
    FIXED_PROFILE
        .into_iter()
        .chain(LIFECYCLE_ONLY_PROFILE)
        .chain(std::iter::once(PENALTY_PROFILE))
}

/// A development build refuses a chain ID naming mainnet or production, so no
/// staging chain can pass for mainnet. A production chain may name mainnet.
pub fn chain_id_allowed(chain_id: &str) -> bool {
    let lower = chain_id.to_ascii_lowercase();
    PRODUCTION || !(lower.contains("mainnet") || lower.contains("production"))
}

/// Refuse a development entry point in a production build.
pub fn development_entry() -> Result<()> {
    ensure!(
        !PRODUCTION,
        "A production build has no development entry points"
    );
    Ok(())
}

/// Refuse to open a chain in a production build until the root-signed
/// production path exists (step A2).
pub fn production_open() -> Result<()> {
    ensure!(
        !PRODUCTION,
        "A production build opens a chain only from a root-signed genesis (production activation step A2)"
    );
    Ok(())
}

#[cfg(test)]
#[path = "build_profile_tests.rs"]
mod tests;
