//! Development release catalog and bounded owned-process observation.
//! File verification and mapping snapshots do not confer production approval.
pub mod component_candidate;
pub mod observation;
// Exact historical implementation is retained only for compatibility tests.
#[cfg(test)]
mod runtime_candidate_v1;

pub mod ownership;

pub mod ownership_security;
