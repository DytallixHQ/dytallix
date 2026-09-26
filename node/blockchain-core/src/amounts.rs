//! Central type definitions and serialization helpers for amounts and gas in Dytallix blockchain.
//!
//! This module provides canonical types for monetary amounts and gas to eliminate
//! u64/u128 mixing issues throughout the codebase.

use serde::{Deserialize, Deserializer, Serializer};

/// Canonical type for all monetary amounts, balances, stakes, and fees.
/// Uses u128 to handle large values without overflow.
pub type Tokens = u128;

/// Canonical type for all gas-related fields and counters.
/// Uses u64 as gas amounts are bounded and for FFI safety with WASM.
pub type Gas = u64;

/// Serde module for serializing u128 values as decimal strings in JSON.
/// This prevents precision loss in JavaScript and other JSON consumers.
pub mod serde_u128_string {
    use super::*;
    use serde::de::Error as DeError;

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<u128>().map_err(D::Error::custom)
    }
}

/// Serde module for optional u128 values serialized as decimal strings.
pub mod serde_opt_u128_string {
    use super::*;
    use serde::de::Error as DeError;

    pub fn serialize<S: Serializer>(v: &Option<u128>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(val) => s.serialize_str(&val.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u128>, D::Error> {
        let opt: Option<String> = Option::deserialize(d)?;
        match opt {
            Some(s) => s.parse::<u128>().map(Some).map_err(D::Error::custom),
            None => Ok(None),
        }
    }
}

/// Serde module for accepting both string and number input (for API compatibility).
/// Always serializes as string to avoid precision issues.
pub mod serde_string_or_number {
    use super::*;
    use serde::de::{self, Visitor};
    use std::fmt;

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        struct U128Visitor;

        impl<'de> Visitor<'de> for U128Visitor {
            type Value = u128;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or number representing u128")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<u128>().map_err(de::Error::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(v as u128)
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
                Ok(v)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v >= 0 {
                    Ok(v as u128)
                } else {
                    Err(de::Error::custom("negative numbers not allowed"))
                }
            }
        }

        d.deserialize_any(U128Visitor)
    }
}

/// Helper function to safely convert Gas (u64) to Tokens (u128) when needed.
pub fn gas_to_tokens(gas: Gas) -> Tokens {
    gas as u128
}

/// Helper function to safely convert Tokens to Gas when the value fits.
/// Returns None if the value is too large for u64.
pub fn tokens_to_gas(tokens: Tokens) -> Option<Gas> {
    if tokens <= u64::MAX as u128 {
        Some(tokens as u64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_u128_string_serde_roundtrip() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestStruct {
            #[serde(with = "serde_u128_string")]
            amount: Tokens,
        }

        let test = TestStruct {
            amount: 12_345_678_901_234_567_890_123_456u128,
        };

        let json = serde_json::to_string(&test).unwrap();
        assert!(json.contains("\"12345678901234567890123456\""));

        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(test, deserialized);
    }

    #[test]
    fn test_opt_u128_string_serde() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestStruct {
            #[serde(with = "serde_opt_u128_string")]
            amount: Option<Tokens>,
        }

        let test_some = TestStruct {
            amount: Some(12345u128),
        };
        let json = serde_json::to_string(&test_some).unwrap();
        assert!(json.contains("\"12345\""));
        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(test_some, deserialized);

        let test_none = TestStruct { amount: None };
        let json = serde_json::to_string(&test_none).unwrap();
        assert!(json.contains("null"));
        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(test_none, deserialized);
    }

    #[test]
    fn test_string_or_number_serde() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestStruct {
            #[serde(with = "serde_string_or_number")]
            amount: Tokens,
        }

        // Test string input
        let json_str = r#"{"amount":"12345"}"#;
        let test: TestStruct = serde_json::from_str(&json_str).unwrap();
        assert_eq!(test.amount, 12345u128);

        // Test number input
        let json_num = r#"{"amount":12345}"#;
        let test: TestStruct = serde_json::from_str(&json_num).unwrap();
        assert_eq!(test.amount, 12345u128);

        // Test serialization always produces string
        let serialized = serde_json::to_string(&test).unwrap();
        assert!(serialized.contains("\"12345\""));
    }

    #[test]
    fn test_gas_tokens_conversion() {
        let gas: Gas = 1000;
        let tokens = gas_to_tokens(gas);
        assert_eq!(tokens, 1000u128);

        let tokens: Tokens = 1000;
        let gas_back = tokens_to_gas(tokens);
        assert_eq!(gas_back, Some(1000u64));

        // Test overflow case
        let large_tokens: Tokens = u64::MAX as u128 + 1;
        let gas_overflow = tokens_to_gas(large_tokens);
        assert_eq!(gas_overflow, None);
    }
}
