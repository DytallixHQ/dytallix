//! The root authority in force (root kit replacement v1, K1; P01, 7 and 8
//! October 2026, `docs/architecture/root-kit-replacement-v1.md`).
//!
//! The upgrade, freeze and resume key sets, by authority epoch, each in force
//! from its start height until the next epoch's. Every emergency, upgrade,
//! handover and restart check takes its keys from the epoch in force at the
//! block it checks, so a replay of history uses the keys each control was
//! signed under. Until a kit replacement writes the record (K2b), there is
//! no record and the configuration's key sets are in force: nothing changes.
//!
//! Only the keys and the epoch come from the record. Thresholds, windows,
//! notices and size bounds stay in the configuration.

use crate::emergency_freeze::{self as emergency, AuthorityPolicy};
use crate::{release_handover as handover, upgrade};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// The record's state key, written by the first kit replacement.
pub const STATE_KEY: &str = "consensus:root-authority:v1";
/// The record's schema.
pub const SCHEMA: u16 = 1;
const MAX_RECORD_BYTES: usize = 1 << 20;
const MAX_EPOCHS: usize = 1024;

/// One authority epoch's key sets and the first height they sign for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Epoch {
    pub authority_epoch: u64,
    pub from_height: u64,
    pub upgrade: AuthorityPolicy,
    pub freeze: AuthorityPolicy,
    pub resume: AuthorityPolicy,
}

/// Every authority epoch, ascending; the last is the newest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: u16,
    pub epochs: Vec<Epoch>,
}

impl Record {
    /// A stored record: canonical, bounded, and its epochs consecutive and
    /// in height order.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_RECORD_BYTES,
            "Root authority record exceeds its bound"
        );
        let record: Self = serde_json::from_slice(bytes)?;
        ensure!(
            serde_json::to_vec(&record)? == bytes,
            "Root authority record is not canonical"
        );
        record.validate()?;
        Ok(record)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == SCHEMA, "Root authority record schema");
        ensure!(
            !self.epochs.is_empty() && self.epochs.len() <= MAX_EPOCHS,
            "Root authority record epoch count"
        );
        for pair in self.epochs.windows(2) {
            ensure!(
                pair[0].authority_epoch.checked_add(1) == Some(pair[1].authority_epoch)
                    && pair[0].from_height < pair[1].from_height,
                "Root authority epochs are not consecutive"
            );
        }
        ensure!(
            self.epochs[0].authority_epoch > 0,
            "Root authority epoch zero"
        );
        Ok(())
    }
    /// The epoch in force at `height`: the last one starting at or before it.
    pub fn at(&self, height: u64) -> Result<&Epoch> {
        self.epochs
            .iter()
            .rev()
            .find(|epoch| epoch.from_height <= height)
            .context("No root authority epoch is in force at this height")
    }
}

/// The stored record, if a kit replacement has written one.
pub fn decode_stored(bytes: Option<&[u8]>) -> Result<Option<Record>> {
    bytes.map(Record::decode).transpose()
}

/// The epoch in force at `height`, or `None` while the configuration's keys
/// are in force.
pub fn at(record: Option<&Record>, height: u64) -> Result<Option<&Epoch>> {
    record.map(|record| record.at(height)).transpose()
}

/// The emergency policy with the epoch's freeze and resume keys.
pub fn emergency_policy<'a>(
    base: &'a emergency::Policy,
    epoch: Option<&Epoch>,
) -> Result<Cow<'a, emergency::Policy>> {
    let Some(epoch) = epoch else {
        return Ok(Cow::Borrowed(base));
    };
    let mut policy = base.clone();
    policy
        .v2
        .as_mut()
        .context("A root authority record needs the schema 2 emergency policy")?
        .authority_epoch = epoch.authority_epoch;
    policy.freeze_authority = epoch.freeze.clone();
    policy.resume_authority = epoch.resume.clone();
    policy.validate()?;
    Ok(Cow::Owned(policy))
}

/// The upgrade policy with the epoch's upgrade keys. Only schema 2 has a
/// replaceable authority.
pub fn upgrade_policy<'a>(
    base: &'a upgrade::Policy,
    epoch: Option<&Epoch>,
) -> Result<Cow<'a, upgrade::Policy>> {
    let Some(epoch) = epoch else {
        return Ok(Cow::Borrowed(base));
    };
    let upgrade::Policy::V2(v2) = base else {
        bail!("A root authority record needs the schema 2 upgrade policy");
    };
    let mut v2 = v2.clone();
    v2.authority = epoch.upgrade.clone();
    v2.authority_epoch = epoch.authority_epoch;
    let policy = upgrade::Policy::V2(v2);
    policy.validate()?;
    Ok(Cow::Owned(policy))
}

/// The release handover policy with the epoch's upgrade keys, which also
/// sign handovers and restarts.
pub fn handover_policy<'a>(
    base: &'a handover::Policy,
    epoch: Option<&Epoch>,
) -> Result<Cow<'a, handover::Policy>> {
    let Some(epoch) = epoch else {
        return Ok(Cow::Borrowed(base));
    };
    ensure!(
        base.v2.is_some(),
        "A root authority record needs the schema 2 handover policy"
    );
    let mut policy = base.clone();
    policy.authority = epoch.upgrade.clone();
    policy.authority_epoch = epoch.authority_epoch;
    policy.validate()?;
    Ok(Cow::Owned(policy))
}

#[cfg(test)]
#[path = "root_authority_tests.rs"]
mod tests;
