use std::fmt;
use std::str::FromStr;

use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};

use crate::error::DytallixError;
use crate::hash::hash_public_key;

const DYTALLIX_HRP: &str = "dytallix";
const DYTALLIX_PREFIX: &str = "dytallix1";
const ADDRESS_HASH_BYTES: usize = 32;
const MLDSA65_PUBLIC_KEY_BYTES: usize = 1_952;
const BECH32_CHARSET: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/// Canonical Dytallix Bech32m address.
///
/// D-Addr values are always built as `Bech32m(BLAKE3(public_key))` with the
/// human-readable part `dytallix`.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::address::DAddr;
/// use dytallix_core::keypair::DytallixKeypair;
///
/// let keypair = DytallixKeypair::generate();
/// let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
/// assert!(addr.as_str().starts_with("dytallix1"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DAddr(String);

impl DAddr {
    /// Derives a canonical Dytallix address from an ML-DSA-65 public key.
    ///
    /// The public key is hashed with BLAKE3 to 32 bytes and then encoded with
    /// Bech32m using the `dytallix` HRP.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::address::DAddr;
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
    /// assert!(addr.as_str().starts_with("dytallix1"));
    /// ```
    pub fn from_public_key(pubkey: &[u8]) -> Result<Self, DytallixError> {
        if pubkey.len() != MLDSA65_PUBLIC_KEY_BYTES {
            return Err(DytallixError::InvalidKeySize {
                expected: MLDSA65_PUBLIC_KEY_BYTES,
                got: pubkey.len(),
            });
        }

        let hash = hash_public_key(pubkey);
        let hrp =
            Hrp::parse(DYTALLIX_HRP).map_err(|err| DytallixError::Bech32Error(err.to_string()))?;
        let encoded = bech32::encode::<Bech32m>(hrp, &hash)
            .map_err(|err| DytallixError::Bech32Error(err.to_string()))?;

        Ok(Self(encoded))
    }

    /// Parses and validates a Dytallix Bech32m address.
    ///
    /// Validation is performed in this order:
    /// 1. The string must start with `dytallix1`.
    /// 2. The data portion must contain only valid Bech32 characters.
    /// 3. The checksum must be a valid Bech32m checksum.
    /// 4. The decoded payload must be exactly 32 bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::address::DAddr;
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// let original = DAddr::from_public_key(keypair.public_key()).unwrap();
    /// let parsed = DAddr::from_str(original.as_str()).unwrap();
    /// assert_eq!(parsed, original);
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, DytallixError> {
        if !s.starts_with(DYTALLIX_PREFIX) {
            return Err(DytallixError::InvalidAddress(
                "must start with dytallix1".to_owned(),
            ));
        }

        let data_part = &s[DYTALLIX_PREFIX.len()..];
        if data_part.is_empty() || !data_part.chars().all(is_valid_bech32_char) {
            return Err(DytallixError::InvalidAddress(
                "contains invalid characters".to_owned(),
            ));
        }

        let checked = CheckedHrpstring::new::<Bech32m>(s).map_err(|_| {
            DytallixError::InvalidAddress("checksum failed — check for typos".to_owned())
        })?;

        if checked.hrp().as_str() != DYTALLIX_HRP {
            return Err(DytallixError::InvalidAddress(
                "must start with dytallix1".to_owned(),
            ));
        }

        let decoded = checked.byte_iter().collect::<Vec<u8>>();
        if decoded.len() != ADDRESS_HASH_BYTES {
            return Err(DytallixError::InvalidAddress(
                "decoded data must be exactly 32 bytes".to_owned(),
            ));
        }

        Ok(Self(s.to_owned()))
    }

    /// Returns the decoded 32-byte address payload.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::address::DAddr;
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
    /// assert_eq!(addr.as_bytes().len(), 32);
    /// ```
    pub fn as_bytes(&self) -> [u8; 32] {
        let checked = CheckedHrpstring::new::<Bech32m>(&self.0)
            .expect("DAddr stores only validated Bech32m addresses");
        let decoded = checked.byte_iter().collect::<Vec<u8>>();

        decoded
            .try_into()
            .expect("DAddr payload is always exactly 32 bytes")
    }

    /// Returns the canonical Bech32m address string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::address::DAddr;
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
    /// assert!(addr.as_str().starts_with("dytallix1"));
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for DAddr {
    type Err = DytallixError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s)
    }
}

fn is_valid_bech32_char(ch: char) -> bool {
    ch.is_ascii() && BECH32_CHARSET.contains(ch)
}
