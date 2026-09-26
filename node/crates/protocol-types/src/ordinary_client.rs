//! Strict public RPC views for ordinary clients. These views contain no secrets.
//! A committed context identifies the node's reported state. It is not a
//! light-client proof or authenticated authority. Clients must check their own
//! expected domain, profile and trusted context before signing.
use crate::{ordinary_fees::FeeProfile, recovery::KeyIdentity};
use serde::{Deserialize, Serialize};
pub const CLIENT_VIEW_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedContext {
    pub chain_id: String,
    #[serde(with = "hex32")]
    pub genesis_digest: [u8; 32],
    #[serde(with = "decimal_u64")]
    pub height: u64,
    #[serde(with = "hex32")]
    pub app_hash: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicOrdinaryConfig {
    pub version: u16,
    pub fee_profile: FeeProfile,
    #[serde(with = "decimal_u64")]
    pub max_state_bytes: u64,
    pub max_grants: u32,
    pub max_receipts: u32,
    pub max_retained_profiles: u32,
    #[serde(with = "decimal_u64")]
    pub max_transport_bytes: u64,
    pub queue_max_entries: u32,
    #[serde(with = "decimal_u64")]
    pub queue_max_wire_bytes: u64,
    #[serde(with = "decimal_u64")]
    pub queue_max_signature_work: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileView {
    pub version: u16,
    pub enabled: bool,
    pub context: CommittedContext,
    #[serde(deserialize_with = "required_option")]
    pub config: Option<PublicOrdinaryConfig>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountDomain {
    pub network: u8,
    pub chain_id: String,
    #[serde(with = "hex32")]
    pub genesis_digest: [u8; 32],
    #[serde(with = "hex32")]
    pub account_id: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountView {
    pub version: u16,
    pub context: CommittedContext,
    pub domain: AccountDomain,
    #[serde(with = "hex32")]
    pub account_id: [u8; 32],
    pub address: String,
    pub current_key: KeyIdentity,
    #[serde(with = "decimal_u64")]
    pub authorization_generation: u64,
    #[serde(with = "decimal_u64")]
    pub spending_nonce: u64,
    pub protected: bool,
    #[serde(with = "hex32")]
    pub profile_digest: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptOutcome {
    Success,
    ApplicationFailure,
    OutOfGas,
}
/// Financial fields use decimal strings in JSON, including values above u64.
/// This DTO does not change the node's durable receipt encoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptView {
    pub version: u16,
    pub context: CommittedContext,
    #[serde(with = "hex32")]
    pub transaction_id: [u8; 32],
    #[serde(with = "hex32")]
    pub envelope_hash: [u8; 32],
    #[serde(with = "hex32")]
    pub actor: [u8; 32],
    #[serde(with = "decimal_u64")]
    pub block_height: u64,
    pub block_index: u32,
    pub contract_version: u16,
    #[serde(with = "decimal_u64")]
    pub profile_version: u64,
    #[serde(with = "hex32")]
    pub profile_digest: [u8; 32],
    pub outcome: ReceiptOutcome,
    #[serde(deserialize_with = "required_option")]
    pub failing_action: Option<u16>,
    #[serde(deserialize_with = "required_option")]
    pub failure_phase: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub rule_class: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub rule_code: Option<String>,
    #[serde(with = "decimal_u64")]
    pub gas_limit: u64,
    #[serde(with = "decimal_u64")]
    pub gas_used: u64,
    #[serde(with = "decimal_u64")]
    pub metadata_gas: u64,
    #[serde(with = "decimal_u128")]
    pub reserved_cap: u128,
    #[serde(with = "decimal_u128")]
    pub charge: u128,
    #[serde(with = "decimal_u128")]
    pub released_cap: u128,
    #[serde(with = "decimal_u64")]
    pub nonce_before: u64,
    #[serde(with = "decimal_u64")]
    pub nonce_after: u64,
}
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
mod hex32 {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(value))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(D::Error::custom("Expected lowercase 64-hex digest"));
        }
        let bytes = hex::decode(value).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| D::Error::custom("Expected 32-byte digest"))
    }
}
macro_rules! decimal {
    ($module:ident, $ty:ty) => {
        mod $module {
            use serde::{de::Error, Deserialize, Deserializer, Serializer};
            pub fn serialize<S: Serializer>(value: &$ty, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&value.to_string())
            }
            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$ty, D::Error> {
                let value = String::deserialize(deserializer)?;
                if value.is_empty()
                    || (value.len() > 1 && value.starts_with('0'))
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(D::Error::custom(
                        "Expected canonical unsigned decimal string",
                    ));
                }
                value.parse::<$ty>().map_err(D::Error::custom)
            }
        }
    };
}
decimal!(decimal_u64, u64);
decimal!(decimal_u128, u128);

#[cfg(test)]
mod tests {
    use super::*;
    fn receipt() -> ReceiptView {
        ReceiptView {
            version: 1,
            context: CommittedContext {
                chain_id: "client-view-test".into(),
                genesis_digest: [1; 32],
                height: u64::MAX,
                app_hash: [2; 32],
            },
            transaction_id: [3; 32],
            envelope_hash: [4; 32],
            actor: [5; 32],
            block_height: u64::MAX,
            block_index: 1,
            contract_version: 1,
            profile_version: u64::MAX,
            profile_digest: [6; 32],
            outcome: ReceiptOutcome::Success,
            failing_action: None,
            failure_phase: None,
            rule_class: None,
            rule_code: None,
            gas_limit: u64::MAX,
            gas_used: u64::MAX,
            metadata_gas: 7,
            reserved_cap: u128::MAX,
            charge: u128::from(u64::MAX) + 1,
            released_cap: u128::MAX - u128::from(u64::MAX) - 1,
            nonce_before: u64::MAX - 1,
            nonce_after: u64::MAX,
        }
    }
    #[test]
    fn client_receipt_preserves_full_width_numbers_without_json_number_conversion() {
        let expected = receipt();
        let bytes = serde_json::to_vec(&expected).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["charge"], "18446744073709551616");
        assert_eq!(value["reserved_cap"], u128::MAX.to_string());
        assert_eq!(value["context"]["height"], u64::MAX.to_string());
        assert_eq!(value["transaction_id"], "03".repeat(32));
        assert_eq!(
            serde_json::from_slice::<ReceiptView>(&bytes).unwrap(),
            expected
        );
    }
    #[test]
    fn client_receipt_rejects_ambiguous_numbers_unknown_fields_and_noncanonical_ids() {
        let original = serde_json::to_value(receipt()).unwrap();
        for bad in [
            serde_json::json!(1),
            serde_json::json!("01"),
            serde_json::json!("+1"),
            serde_json::json!("-1"),
            serde_json::json!("1.0"),
            serde_json::json!("340282366920938463463374607431768211456"),
        ] {
            let mut value = original.clone();
            value["charge"] = bad;
            assert!(serde_json::from_value::<ReceiptView>(value).is_err());
        }
        let mut unknown = original.clone();
        unknown["authority_proven"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ReceiptView>(unknown).is_err());
        let mut nested = original.clone();
        nested["context"]["trusted"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ReceiptView>(nested).is_err());
        let mut upper = original.clone();
        upper["transaction_id"] = serde_json::json!("AB".repeat(32));
        assert!(serde_json::from_value::<ReceiptView>(upper).is_err());
        let mut height = original.clone();
        height["block_height"] = serde_json::json!("18446744073709551616");
        assert!(serde_json::from_value::<ReceiptView>(height).is_err());
        let raw = serde_json::to_string(&receipt()).unwrap();
        let duplicate = raw.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
        assert!(serde_json::from_str::<ReceiptView>(&duplicate).is_err());
    }
    #[test]
    fn client_account_and_profile_require_explicit_fields_and_canonical_context() {
        let context = receipt().context;
        let account = AccountView {
            version: 1,
            context: context.clone(),
            domain: AccountDomain {
                network: 3,
                chain_id: context.chain_id.clone(),
                genesis_digest: context.genesis_digest,
                account_id: [3; 32],
            },
            account_id: [3; 32],
            address: "synthetic-public-address".into(),
            current_key: KeyIdentity {
                algorithm: "mldsa87".into(),
                public_key: vec![7; 2592],
            },
            authorization_generation: u64::MAX,
            spending_nonce: u64::MAX,
            protected: true,
            profile_digest: [8; 32],
        };
        let value = serde_json::to_value(&account).unwrap();
        assert_eq!(value["authorization_generation"], u64::MAX.to_string());
        assert_eq!(
            serde_json::from_value::<AccountView>(value.clone()).unwrap(),
            account
        );
        let mut wrong = value.clone();
        wrong["spending_nonce"] = serde_json::json!(1);
        assert!(serde_json::from_value::<AccountView>(wrong).is_err());
        let mut unknown = value.clone();
        unknown["domain"]["trusted"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AccountView>(unknown).is_err());
        let mut key = value.clone();
        key["current_key"]["private_key"] = serde_json::json!("not permitted");
        assert!(serde_json::from_value::<AccountView>(key).is_err());
        let profile = ProfileView {
            version: 1,
            enabled: false,
            context,
            config: None,
        };
        let mut value = serde_json::to_value(&profile).unwrap();
        assert_eq!(
            serde_json::from_value::<ProfileView>(value.clone()).unwrap(),
            profile
        );
        value.as_object_mut().unwrap().remove("config");
        assert!(serde_json::from_value::<ProfileView>(value).is_err());
        let mut receipt = serde_json::to_value(receipt()).unwrap();
        receipt.as_object_mut().unwrap().remove("failure_phase");
        assert!(serde_json::from_value::<ReceiptView>(receipt).is_err());
    }
}
