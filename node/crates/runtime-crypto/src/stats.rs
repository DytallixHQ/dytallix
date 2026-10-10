//! Signature verification counts and time, for the node's metrics (metrics
//! v1; P01, 10 October 2026). Process-wide counters that every ML-DSA
//! verification in this crate updates; they never change a result.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

static VALID: AtomicU64 = AtomicU64::new(0);
static INVALID: AtomicU64 = AtomicU64::new(0);
static NANOS: AtomicU64 = AtomicU64::new(0);

/// ML-DSA verifications since the process started: valid and invalid
/// signatures, and the total time spent verifying them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Verifications {
    pub valid: u64,
    pub invalid: u64,
    pub nanos: u64,
}

/// One cryptographic verification, timed and counted. Its result is
/// returned unchanged. Malformed keys and signatures are refused before it
/// and are not counted.
pub(crate) fn measured(verify: impl FnOnce() -> bool) -> bool {
    let started = Instant::now();
    let valid = verify();
    let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    NANOS.fetch_add(nanos, Relaxed);
    if valid { &VALID } else { &INVALID }.fetch_add(1, Relaxed);
    valid
}

pub fn verifications() -> Verifications {
    Verifications {
        valid: VALID.load(Relaxed),
        invalid: INVALID.load(Relaxed),
        nanos: NANOS.load(Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_verification_is_counted_and_its_result_kept() {
        let before = verifications();
        assert!(measured(|| true));
        assert!(!measured(|| false));
        let after = verifications();
        assert!(after.valid > before.valid && after.invalid > before.invalid);
        assert!(after.nanos >= before.nanos);
    }
}
