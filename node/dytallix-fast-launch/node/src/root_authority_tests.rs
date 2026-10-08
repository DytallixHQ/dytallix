use super::*;
use crate::consensus_settlement::ConsensusConfig;
use crate::emergency_freeze::AuthorityKey;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// The rehearsal configuration this build accepts (A7).
fn config() -> ConsensusConfig {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/mainnet-preparation/fixtures")
        .join(if crate::build_profile::PRODUCTION {
            "genesis-production-rehearsal"
        } else {
            "genesis-rehearsal"
        })
        .join("application-config.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn key(seed: u8) -> AuthorityKey {
    let public = [seed; 64];
    AuthorityKey {
        key_id: hex::encode(Sha256::digest(public)),
        public_key_hex: hex::encode(public),
    }
}

/// Seat 1 of each set replaced, as a kit replacement does.
fn replaced(authority: &AuthorityPolicy, seed: u8) -> AuthorityPolicy {
    let mut authority = authority.clone();
    authority.keys[0] = key(seed);
    authority.keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    authority
}

fn record(config: &ConsensusConfig) -> Record {
    let emergency = config.emergency.as_ref().unwrap();
    let upgrade = config.upgrade.as_ref().unwrap().authority().clone();
    let first = Epoch {
        authority_epoch: 1,
        from_height: 1,
        upgrade: upgrade.clone(),
        freeze: emergency.freeze_authority.clone(),
        resume: emergency.resume_authority.clone(),
    };
    let second = Epoch {
        authority_epoch: 2,
        from_height: 100,
        upgrade: replaced(&upgrade, 201),
        freeze: replaced(&emergency.freeze_authority, 202),
        resume: replaced(&emergency.resume_authority, 203),
    };
    Record {
        schema: 1,
        epochs: vec![first, second],
    }
}

#[test]
fn without_a_record_the_configuration_is_in_force() {
    let config = config();
    let (emergency, upgrade, handover) = (
        config.emergency.as_ref().unwrap(),
        config.upgrade.as_ref().unwrap(),
        config.release_handover.as_ref().unwrap(),
    );
    assert_eq!(decode_stored(None).unwrap(), None);
    let epoch = at(None, 1_000_000).unwrap();
    assert!(
        matches!(emergency_policy(emergency, epoch).unwrap(), Cow::Borrowed(p) if std::ptr::eq(p, emergency))
    );
    assert!(
        matches!(upgrade_policy(upgrade, epoch).unwrap(), Cow::Borrowed(p) if std::ptr::eq(p, upgrade))
    );
    assert!(
        matches!(handover_policy(handover, epoch).unwrap(), Cow::Borrowed(p) if std::ptr::eq(p, handover))
    );
    // The configuration's epochs agree, as the configuration check requires.
    assert_eq!(
        emergency.v2.as_ref().unwrap().authority_epoch,
        upgrade.authority_epoch()
    );
    assert_eq!(handover.authority_epoch, upgrade.authority_epoch());
}

#[test]
fn each_height_takes_the_epoch_in_force() {
    let config = config();
    let record = record(&config);
    let stored = record.encode().unwrap();
    assert_eq!(
        decode_stored(Some(&stored)).unwrap().as_ref(),
        Some(&record)
    );
    assert_eq!(record.at(1).unwrap().authority_epoch, 1);
    assert_eq!(record.at(99).unwrap().authority_epoch, 1);
    assert_eq!(record.at(100).unwrap().authority_epoch, 2);
    assert!(record.at(0).is_err());

    let second = record.at(100).unwrap();
    let emergency = emergency_policy(config.emergency.as_ref().unwrap(), Some(second)).unwrap();
    assert_eq!(emergency.freeze_authority, second.freeze);
    assert_eq!(emergency.resume_authority, second.resume);
    assert_eq!(emergency.v2.as_ref().unwrap().authority_epoch, 2);
    let upgrade = upgrade_policy(config.upgrade.as_ref().unwrap(), Some(second)).unwrap();
    assert_eq!(
        (upgrade.authority(), upgrade.authority_epoch()),
        (&second.upgrade, 2)
    );
    let handover =
        handover_policy(config.release_handover.as_ref().unwrap(), Some(second)).unwrap();
    assert_eq!(
        (&handover.authority, handover.authority_epoch),
        (&second.upgrade, 2)
    );
    // Everything but the keys and the epoch stays the configuration's.
    let mut back = emergency.into_owned();
    back.freeze_authority = config.emergency.as_ref().unwrap().freeze_authority.clone();
    back.resume_authority = config.emergency.as_ref().unwrap().resume_authority.clone();
    back.v2.as_mut().unwrap().authority_epoch = 1;
    assert_eq!(&back, config.emergency.as_ref().unwrap());
}

#[test]
fn a_malformed_record_is_refused() {
    let config = config();
    let good = record(&config);
    let mut cases: Vec<Record> = Vec::new();
    let mut schema = good.clone();
    schema.schema = 2;
    cases.push(schema);
    cases.push(Record {
        schema: 1,
        epochs: Vec::new(),
    });
    let mut gap = good.clone();
    gap.epochs[1].authority_epoch = 3;
    cases.push(gap);
    let mut backwards = good.clone();
    backwards.epochs[1].from_height = 1;
    cases.push(backwards);
    let mut zero = good.clone();
    zero.epochs.truncate(1);
    zero.epochs[0].authority_epoch = 0;
    cases.push(zero);
    for bad in cases {
        assert!(bad.encode().is_err(), "{bad:?}");
        assert!(Record::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    // Stored bytes are canonical.
    let mut pretty = serde_json::to_vec_pretty(&good).unwrap();
    assert!(Record::decode(&pretty).is_err());
    pretty.truncate(3);
    assert!(Record::decode(&pretty).is_err());
}

#[test]
fn the_configuration_keeps_one_authority_epoch() {
    let mut config = config();
    config.validate().unwrap();
    config
        .emergency
        .as_mut()
        .unwrap()
        .v2
        .as_mut()
        .unwrap()
        .authority_epoch = 2;
    let error = format!("{:#}", config.validate().unwrap_err());
    assert!(error.contains("authority epochs differ"), "{error}");
}
