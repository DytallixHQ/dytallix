//! Exact signature algorithm identifiers shared by every account wire format.
//!
//! An entry makes an algorithm encodable in the ordinary, recovery, sponsor and
//! fee codecs. It does not approve the algorithm for mainnet and it does not
//! supply a verifier: `dytallix-runtime-crypto` owns both decisions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactAlgorithm {
    pub name: &'static str,
    pub code: u16,
    pub public_key_len: usize,
    pub signature_len: usize,
}

/// FIPS 204 ML-DSA-65.
pub const MLDSA65: ExactAlgorithm = ExactAlgorithm {
    name: "mldsa65",
    code: 1,
    public_key_len: 1952,
    signature_len: 3309,
};

/// FIPS 204 ML-DSA-87. Encodable only; no verifier accepts it.
pub const MLDSA87: ExactAlgorithm = ExactAlgorithm {
    name: "mldsa87",
    code: 2,
    public_key_len: 2592,
    signature_len: 4627,
};

/// FIPS 205 SLH-DSA-SHAKE-192s, pure mode. Category 3, matching ML-DSA-65.
/// Not the legacy SPHINCS+-SHAKE-192s-simple key, which has the same lengths.
pub const SLHDSA_SHAKE_192S: ExactAlgorithm = ExactAlgorithm {
    name: "slhdsa-shake-192s",
    code: 3,
    public_key_len: 48,
    signature_len: 16224,
};

pub const ALL: &[ExactAlgorithm] = &[MLDSA65, MLDSA87, SLHDSA_SHAKE_192S];

/// Largest encodable public key, for fixed key-ID buffers.
pub const MAX_PUBLIC_KEY_LEN: usize = 2592;

/// Account signature algorithms mainnet accepts (D06-Q01: ML-DSA-65 only).
/// Every mainnet check reads this list; changing it needs a root-signed upgrade.
pub const MAINNET_APPROVED: &[&str] = &[MLDSA65.name];

pub fn mainnet_approved(name: &str) -> bool {
    MAINNET_APPROVED.contains(&name)
}

pub fn by_name(name: &str) -> Option<&'static ExactAlgorithm> {
    ALL.iter().find(|a| a.name == name)
}

pub fn by_code(code: u16) -> Option<&'static ExactAlgorithm> {
    ALL.iter().find(|a| a.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_unique_and_bounded() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(
                a.code > 0 && a.code <= u16::from(u8::MAX),
                "recovery codecs use a u8 code"
            );
            assert!(a.public_key_len <= MAX_PUBLIC_KEY_LEN);
            for b in &ALL[i + 1..] {
                assert_ne!(a.name, b.name);
                assert_ne!(a.code, b.code);
            }
        }
    }
}
