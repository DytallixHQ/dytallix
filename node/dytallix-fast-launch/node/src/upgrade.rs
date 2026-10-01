//! Version registry for retained chain upgrade implementations, and the
//! versioned interface the consensus adapter uses. Existing history always
//! uses its original implementation and wire format: v1 (development
//! qualification) and v2 (production activation v1, step A3), each
//! hash-pinned here and in the registry.
use crate::emergency_freeze as emergency;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[path = "upgrade/v1/upgrade.rs"]
pub mod v1;
#[path = "upgrade/v2/upgrade.rs"]
pub mod v2;
/// Shared by both versions: the signature verifier and the digest index the
/// one compiled migration writes.
pub use v1::{index_key, Verifier, INDEX_PREFIX, INDEX_STATE_KEY, MIGRATION_ID};

const REGISTRY: &[u8] = include_bytes!("upgrade/registry.json");
const V1_SOURCE: &[u8] = include_bytes!("upgrade/v1/upgrade.rs");
const V1_SHA256: &str = "57bf05eda1f4676ceabc6b8b58e71e513970feb340a99d62ab75d6d1dafe31eb";
const V2_SOURCE: &[u8] = include_bytes!("upgrade/v2/upgrade.rs");
const V2_SHA256: &str = "14d103c3bf26849be10864d160e957bbc9cd77f40a08b0736d6f59f0bff8847f";

/// Exact registry identity for candidate manifests. Registry entries are compiled.
pub fn registry_sha256() -> String {
    hex::encode(Sha256::digest(REGISTRY))
}

/// Reject a changed retained implementation rather than silently changing history.
pub fn validate_registry() -> Result<()> {
    ensure!(
        hex::encode(Sha256::digest(V1_SOURCE)) == V1_SHA256,
        "Retained upgrade v1 implementation changed"
    );
    ensure!(
        v1::migration_sha256() == V1_SHA256,
        "Retained upgrade v1 digest changed"
    );
    ensure!(
        hex::encode(Sha256::digest(V2_SOURCE)) == V2_SHA256,
        "Retained upgrade v2 implementation changed"
    );
    ensure!(
        v2::migration_sha256() == V2_SHA256,
        "Retained upgrade v2 digest changed"
    );
    let entry = |version: u16, sha256: &str| {
        serde_json::json!({"id":MIGRATION_ID,"version":version,"implementation_sha256":sha256,
            "source_schema":0,"target_schema":1})
    };
    let expected =
        serde_json::json!({"schema":1,"migrations":[entry(1, V1_SHA256), entry(2, V2_SHA256)]});
    ensure!(
        serde_json::from_slice::<serde_json::Value>(REGISTRY)? == expected,
        "Compiled migration registry differs from retained implementations"
    );
    Ok(())
}

/// An upgrade policy of either retained version, chosen by its `schema`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Policy {
    V1(v1::Policy),
    V2(v2::Policy),
}
impl<'de> Deserialize<'de> for Policy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let fields = deserializer.deserialize_map(StrictMap)?;
        let schema = fields.get("schema").and_then(serde_json::Value::as_u64);
        let value = serde_json::Value::Object(fields);
        match schema {
            Some(1) => v1::Policy::deserialize(value)
                .map(Policy::V1)
                .map_err(D::Error::custom),
            Some(2) => v2::Policy::deserialize(value)
                .map(Policy::V2)
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom("Unsupported upgrade policy schema")),
        }
    }
}
/// A JSON object with no repeated field, so a policy decodes exactly.
struct StrictMap;
impl<'de> serde::de::Visitor<'de> for StrictMap {
    type Value = serde_json::Map<String, serde_json::Value>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("an upgrade policy object")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut access: A,
    ) -> Result<Self::Value, A::Error> {
        let mut map = serde_json::Map::new();
        while let Some((key, value)) = access.next_entry::<String, serde_json::Value>()? {
            if map.insert(key.clone(), value).is_some() {
                return Err(serde::de::Error::custom(format!(
                    "duplicate upgrade policy field {key}"
                )));
            }
        }
        Ok(map)
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::V1(p) => p.validate(),
            Self::V2(p) => p.validate(),
        }
    }
    pub fn sha256(&self) -> Result<String> {
        match self {
            Self::V1(p) => p.sha256(),
            Self::V2(p) => p.sha256(),
        }
    }
    pub fn chain_id(&self) -> &str {
        match self {
            Self::V1(p) => &p.chain_id,
            Self::V2(p) => &p.chain_id,
        }
    }
    pub fn genesis_sha256(&self) -> &str {
        match self {
            Self::V1(p) => &p.genesis_sha256,
            Self::V2(p) => &p.genesis_sha256,
        }
    }
    /// The genesis release.
    pub fn initial_release_sha512(&self) -> &str {
        match self {
            Self::V1(p) => &p.source_release_sha512,
            Self::V2(p) => &p.initial_release_sha512,
        }
    }
    pub fn authority(&self) -> &emergency::AuthorityPolicy {
        match self {
            Self::V1(p) => &p.authority,
            Self::V2(p) => &p.authority,
        }
    }
    pub fn authority_epoch(&self) -> u64 {
        match self {
            Self::V1(p) => p.authority_epoch,
            Self::V2(p) => p.authority_epoch,
        }
    }
    pub fn max_control_bytes(&self) -> usize {
        match self {
            Self::V1(p) => p.max_control_bytes,
            Self::V2(p) => p.max_control_bytes,
        }
    }
    /// Schema 2 signs over an anchored window; schema 1 binds one height.
    pub fn max_anchor_age_blocks(&self) -> Option<u64> {
        match self {
            Self::V1(_) => None,
            Self::V2(p) => Some(p.max_anchor_age_blocks),
        }
    }
    /// The digest of the implementation that runs this policy's migration.
    pub fn migration_sha256(&self) -> String {
        match self {
            Self::V1(_) => v1::migration_sha256(),
            Self::V2(_) => v2::migration_sha256(),
        }
    }
    pub fn state_key(&self) -> &'static str {
        match self {
            Self::V1(_) => v1::STATE_KEY,
            Self::V2(_) => v2::STATE_KEY,
        }
    }
    pub fn receipt_key(&self, sequence: u64) -> String {
        match self {
            Self::V1(_) => v1::receipt_key(sequence),
            Self::V2(_) => v2::receipt_key(sequence),
        }
    }
    pub fn as_v1(&self) -> Option<&v1::Policy> {
        match self {
            Self::V1(p) => Some(p),
            Self::V2(_) => None,
        }
    }
    pub fn as_v2(&self) -> Option<&v2::Policy> {
        match self {
            Self::V1(_) => None,
            Self::V2(p) => Some(p),
        }
    }
}

/// Committed upgrade state of the policy's version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    V1(v1::State),
    V2(v2::State),
}
impl State {
    pub fn new(policy: &Policy) -> Result<Self> {
        Ok(match policy {
            Policy::V1(p) => Self::V1(v1::State::new(p)?),
            Policy::V2(p) => Self::V2(v2::State::new(p)?),
        })
    }
    pub fn active_schema(&self) -> u16 {
        match self {
            Self::V1(s) => s.active_schema(),
            Self::V2(s) => s.active_schema(),
        }
    }
    pub fn next_sequence(&self) -> u64 {
        match self {
            Self::V1(s) => s.next_sequence(),
            Self::V2(s) => s.next_sequence(),
        }
    }
    pub fn last_receipt_sha256(&self) -> Option<&str> {
        match self {
            Self::V1(s) => s.last_receipt_sha256(),
            Self::V2(s) => s.last_receipt_sha256(),
        }
    }
    pub fn activation_receipt_sha256(&self) -> Option<&str> {
        match self {
            Self::V1(s) => s.activation_receipt_sha256(),
            Self::V2(s) => s.activation_receipt_sha256(),
        }
    }
    /// v1 binds the genesis release; v2 follows the handover state.
    pub fn active_release_sha512(&self) -> Option<&str> {
        match self {
            Self::V1(s) => Some(s.active_release_sha512()),
            Self::V2(_) => None,
        }
    }
    pub fn as_v1(&self) -> Option<&v1::State> {
        match self {
            Self::V1(s) => Some(s),
            Self::V2(_) => None,
        }
    }
    pub fn as_v2(&self) -> Option<&v2::State> {
        match self {
            Self::V1(_) => None,
            Self::V2(s) => Some(s),
        }
    }
    /// The pending plan, as the status query reports it.
    pub fn pending_json(&self) -> Result<serde_json::Value> {
        Ok(match self {
            Self::V1(s) => serde_json::to_value(s.pending())?,
            Self::V2(s) => serde_json::to_value(s.pending())?,
        })
    }
}
pub fn encode_state(state: &State) -> Result<Vec<u8>> {
    match state {
        State::V1(s) => v1::encode_state(s),
        State::V2(s) => v2::encode_state(s),
    }
}
pub fn decode_state(policy: &Policy, bytes: &[u8]) -> Result<State> {
    Ok(match policy {
        Policy::V1(p) => State::V1(v1::decode_state(p, bytes)?),
        Policy::V2(p) => State::V2(v2::decode_state(p, bytes)?),
    })
}

/// A committed upgrade receipt of the policy's version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Receipt {
    V1(v1::Receipt),
    V2(v2::Receipt),
}
impl Receipt {
    pub fn sha256(&self) -> Result<String> {
        match self {
            Self::V1(r) => r.sha256(),
            Self::V2(r) => r.sha256(),
        }
    }
    pub fn sequence(&self) -> u64 {
        match self {
            Self::V1(r) => r.sequence(),
            Self::V2(r) => r.sequence(),
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        match self {
            Self::V1(r) => v1::encode_receipt(r),
            Self::V2(r) => v2::encode_receipt(r),
        }
    }
    /// The canonical bytes of the recorded control.
    pub fn control_bytes(&self) -> Result<Vec<u8>> {
        Ok(match self {
            Self::V1(r) => serde_json::to_vec(&r.control)?,
            Self::V2(r) => serde_json::to_vec(&r.control)?,
        })
    }
    /// The finalized anchor a v2 control named.
    pub fn anchor_height(&self) -> Option<u64> {
        match self {
            Self::V1(_) => None,
            Self::V2(r) => Some(r.control.payload.anchor_height),
        }
    }
}
pub fn decode_receipt(policy: &Policy, bytes: &[u8]) -> Result<Receipt> {
    Ok(match policy {
        Policy::V1(p) => Receipt::V1(v1::decode_receipt(p, bytes)?),
        Policy::V2(p) => Receipt::V2(v2::decode_receipt(p, bytes)?),
    })
}

/// The canonical bytes of a control this policy decodes.
pub fn canonical_control(policy: &Policy, raw: &[u8]) -> Result<Vec<u8>> {
    Ok(match policy {
        Policy::V1(p) => serde_json::to_vec(&v1::decode_control(p, raw)?)?,
        Policy::V2(p) => serde_json::to_vec(&v2::decode_control(p, raw)?)?,
    })
}
pub fn control_sequence(policy: &Policy, raw: &[u8]) -> Result<u64> {
    Ok(match policy {
        Policy::V1(p) => v1::decode_control(p, raw)?.payload.sequence,
        Policy::V2(p) => v2::decode_control(p, raw)?.payload.sequence,
    })
}
/// The finalized anchor height a v2 control names; None for v1.
pub fn control_anchor_height(policy: &Policy, raw: &[u8]) -> Result<Option<u64>> {
    Ok(match policy {
        Policy::V1(p) => {
            v1::decode_control(p, raw)?;
            None
        }
        Policy::V2(p) => Some(v2::decode_control(p, raw)?.payload.anchor_height),
    })
}
pub fn is_rejection(error: &anyhow::Error) -> bool {
    v1::is_rejection(error) || v2::is_rejection(error)
}

/// Migration writes prepared in the activation block.
#[derive(Clone, Debug)]
pub struct Migration {
    pub writes: BTreeMap<Vec<u8>, Vec<u8>>,
    pub receipt_count: u64,
    pub source_digest: String,
    pub resulting_index_digest: String,
}
#[derive(Clone, Debug)]
pub struct BlockPlan {
    pub state: State,
    pub receipt: Option<Receipt>,
    pub migration: Option<Migration>,
}
impl From<v1::BlockPlan> for BlockPlan {
    fn from(plan: v1::BlockPlan) -> Self {
        Self {
            state: State::V1(plan.state),
            receipt: plan.receipt.map(Receipt::V1),
            migration: plan.migration.map(|m| Migration {
                writes: m.writes,
                receipt_count: m.receipt_count,
                source_digest: m.source_digest,
                resulting_index_digest: m.resulting_index_digest,
            }),
        }
    }
}
impl From<v2::BlockPlan> for BlockPlan {
    fn from(plan: v2::BlockPlan) -> Self {
        Self {
            state: State::V2(plan.state),
            receipt: plan.receipt.map(Receipt::V2),
            migration: plan.migration.map(|m| Migration {
                writes: m.writes,
                receipt_count: m.receipt_count,
                source_digest: m.source_digest,
                resulting_index_digest: m.resulting_index_digest,
            }),
        }
    }
}

/// Facts about the block the adapter reads from committed state. v2 also
/// needs the active release and, with a control, the anchor it names.
#[derive(Clone, Debug)]
pub struct Context {
    pub height: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    pub active_release_sha512: String,
    pub finalized_anchor: Option<emergency::FinalizedAnchor>,
    pub emergency_frozen: bool,
    pub emergency_control_present: bool,
    pub emergency_upgrade_hold: bool,
    pub emergency_receipt_sha256: Option<String>,
}
impl Context {
    fn v1(&self) -> Result<v1::BlockContext> {
        ensure!(
            self.finalized_anchor.is_none(),
            "A v1 upgrade context carries no anchor"
        );
        Ok(v1::BlockContext {
            height: self.height,
            parent_height: self.parent_height,
            parent_app_hash: self.parent_app_hash.clone(),
            emergency_frozen: self.emergency_frozen,
            emergency_control_present: self.emergency_control_present,
            emergency_upgrade_hold: self.emergency_upgrade_hold,
            emergency_receipt_sha256: self.emergency_receipt_sha256.clone(),
        })
    }
    fn v2(&self) -> v2::BlockContext {
        v2::BlockContext {
            height: self.height,
            parent_height: self.parent_height,
            parent_app_hash: self.parent_app_hash.clone(),
            active_release_sha512: self.active_release_sha512.clone(),
            finalized_anchor: self.finalized_anchor.clone(),
            emergency_frozen: self.emergency_frozen,
            emergency_control_present: self.emergency_control_present,
            emergency_upgrade_hold: self.emergency_upgrade_hold,
            emergency_receipt_sha256: self.emergency_receipt_sha256.clone(),
        }
    }
}

/// The committed emergency history before the block, from which each
/// version builds its own verified view for the migration.
pub struct EmergencyHistory<'a> {
    pub policy: &'a emergency::Policy,
    pub records: &'a [(emergency::Receipt, emergency::BlockContext)],
    pub state: &'a emergency::State,
}

pub fn plan_block(
    policy: &Policy,
    state: &State,
    context: &Context,
    raw: Option<&[u8]>,
    history: &EmergencyHistory<'_>,
    verifier: &dyn Verifier,
) -> Result<BlockPlan> {
    Ok(match (policy, state) {
        (Policy::V1(p), State::V1(s)) => v1::plan_block(
            p,
            s,
            &context.v1()?,
            raw,
            &v1::VerifiedEmergencyHistory::from_records(
                history.policy,
                history.records,
                history.state,
            )?,
            verifier,
        )?
        .into(),
        (Policy::V2(p), State::V2(s)) => v2::plan_block(
            p,
            s,
            &context.v2(),
            raw,
            &v2::VerifiedEmergencyHistory::from_records(
                history.policy,
                history.records,
                history.state,
            )?,
            verifier,
        )?
        .into(),
        _ => anyhow::bail!("Upgrade state version differs from policy"),
    })
}

pub fn replay_record(
    policy: &Policy,
    state: &State,
    receipt: &Receipt,
    context: &Context,
    history: &EmergencyHistory<'_>,
    verifier: Option<&dyn Verifier>,
) -> Result<BlockPlan> {
    Ok(match (policy, state, receipt) {
        (Policy::V1(p), State::V1(s), Receipt::V1(r)) => v1::replay_record(
            p,
            s,
            r,
            &context.v1()?,
            &v1::VerifiedEmergencyHistory::from_records(
                history.policy,
                history.records,
                history.state,
            )?,
            verifier,
        )?
        .into(),
        (Policy::V2(p), State::V2(s), Receipt::V2(r)) => v2::replay_record(
            p,
            s,
            r,
            &context.v2(),
            &v2::VerifiedEmergencyHistory::from_records(
                history.policy,
                history.records,
                history.state,
            )?,
            verifier,
        )?
        .into(),
        _ => anyhow::bail!("Upgrade receipt version differs from policy"),
    })
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    #[test]
    fn retained_implementations_and_registry_match() {
        validate_registry().unwrap();
        assert_eq!(v1::migration_sha256(), V1_SHA256);
        assert_eq!(v2::migration_sha256(), V2_SHA256);
        assert_eq!(registry_sha256().len(), 64);
    }
    #[test]
    fn policy_schema_selects_the_version_and_keeps_v1_bytes() {
        let v1 = serde_json::json!({"schema":1,"development_only":true,"chain_id":"c",
            "genesis_sha256":"aa".repeat(32),"source_release_sha512":"bb".repeat(64),
            "authority_epoch":1,"authority":{"keys":[{"key_id":"k","public_key_hex":"11".repeat(64)}],"threshold":1},
            "initial_sequence":1,"max_control_bytes":1000,"max_signatures":1,
            "migration_bounds":{"max_receipts":1,"max_receipt_bytes":1,"max_write_bytes":1}});
        let raw = serde_json::to_vec(&v1).unwrap();
        let policy: Policy = serde_json::from_slice(&raw).unwrap();
        assert!(matches!(policy, Policy::V1(_)));
        let direct: v1::Policy = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            serde_json::to_vec(&policy).unwrap(),
            serde_json::to_vec(&direct).unwrap()
        );
        let mut other = v1.clone();
        other["schema"] = 3.into();
        assert!(serde_json::from_value::<Policy>(other).is_err());
        let mut v2_shaped = v1.clone();
        v2_shaped["schema"] = 2.into();
        assert!(serde_json::from_value::<Policy>(v2_shaped).is_err());
        let repeated = br#"{"schema":1,"schema":1}"#;
        let error = serde_json::from_slice::<Policy>(repeated).unwrap_err();
        assert!(error.to_string().contains("duplicate"), "{error}");
    }
}
