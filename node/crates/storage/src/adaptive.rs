//! Durable command journal. It does not mint or allocate tokens.
//! The legacy adapter holds an exclusive storage borrow and locks each write.
//! Read-only preparation does not lock or mutate storage. Combined writers must
//! hold this storage's execution guard through checked append and final commit.
use crate::state::Storage;
use dytallix_adaptive_emission::{
    Command, Config, Controller, Observation, Snapshot, MAX_ENCODED_LEN,
};
use rocksdb::{Direction, IteratorMode, WriteBatch, WriteOptions, DB};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const HEAD_KEY: &[u8] = b"adaptive:v1:head";
const EVENT_PREFIX: &[u8] = b"adaptive:v1:event:";
const HEAD_MAGIC: &[u8; 8] = b"DYTAEH01";
const EVENT_MAGIC: &[u8; 8] = b"DYTAEJ01";

#[derive(Debug)]
pub enum JournalError {
    Storage(rocksdb::Error),
    Controller(dytallix_adaptive_emission::Error),
    MissingCheckpoint,
    AlreadyInitialized,
    InvalidCheckpoint,
    InvalidRecord,
    BindingMismatch,
    ConfigMismatch,
    RecordConflict,
    AuditLimit,
    StalePreparation,
    ExecutionLock,
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "adaptive journal: {self:?}")
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Controller(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rocksdb::Error> for JournalError {
    fn from(value: rocksdb::Error) -> Self {
        Self::Storage(value)
    }
}
impl From<dytallix_adaptive_emission::Error> for JournalError {
    fn from(value: dytallix_adaptive_emission::Error) -> Self {
        Self::Controller(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedCommand {
    pub observation: Observation,
    pub for_epoch: u64,
    pub emission_udrt: u64,
    pub before_digest: [u8; 32],
    pub after_digest: [u8; 32],
}

/// Holds a mutable storage borrow. Do not bypass this adapter to write its keys.
/// `binding` must identify the caller's approved genesis/protocol context.
pub struct AdaptiveJournal<'a> {
    storage: &'a mut Storage,
    binding: [u8; 32],
    config: Config,
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn event_key(epoch: u64) -> Vec<u8> {
    let mut key = EVENT_PREFIX.to_vec();
    key.extend_from_slice(&epoch.to_be_bytes());
    key
}
fn write_sync(db: &DB, batch: WriteBatch) -> Result<(), JournalError> {
    let mut options = WriteOptions::default();
    options.set_sync(true);
    db.write_opt(batch, &options)?;
    Ok(())
}

fn encode_head(binding: &[u8; 32], controller: &Controller) -> (Vec<u8>, [u8; 32]) {
    let mut bytes = HEAD_MAGIC.to_vec();
    bytes.extend_from_slice(binding);
    bytes.extend_from_slice(&controller.encode());
    let hash = digest(&bytes);
    bytes.extend_from_slice(&hash);
    (bytes, hash)
}

impl RecordedCommand {
    fn encode(&self, binding: &[u8; 32]) -> Vec<u8> {
        let mut bytes = EVENT_MAGIC.to_vec();
        bytes.extend_from_slice(binding);
        for value in [
            self.observation.epoch,
            self.observation.utilization_ppm,
            self.observation.volatility_ppm,
            self.for_epoch,
            self.emission_udrt,
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&self.before_digest);
        bytes.extend_from_slice(&self.after_digest);
        let hash = digest(&bytes);
        bytes.extend_from_slice(&hash);
        bytes
    }

    fn decode(bytes: &[u8], binding: &[u8; 32]) -> Result<Self, JournalError> {
        if bytes.len() != 176
            || &bytes[..8] != EVENT_MAGIC
            || digest(&bytes[..144]).as_slice() != &bytes[144..]
        {
            return Err(JournalError::InvalidRecord);
        }
        if &bytes[8..40] != binding {
            return Err(JournalError::BindingMismatch);
        }
        // Fixed length above proves that these slices contain eight bytes each.
        let value = |offset| {
            let mut b = [0; 8];
            b.copy_from_slice(&bytes[offset..offset + 8]);
            u64::from_be_bytes(b)
        };
        let observation = Observation {
            epoch: value(40),
            utilization_ppm: value(48),
            volatility_ppm: value(56),
        };
        let for_epoch = value(64);
        if observation.epoch.checked_add(1) != Some(for_epoch)
            || observation.utilization_ppm > dytallix_adaptive_emission::SCALE
        {
            return Err(JournalError::InvalidRecord);
        }
        let mut before_digest = [0; 32];
        before_digest.copy_from_slice(&bytes[80..112]);
        let mut after_digest = [0; 32];
        after_digest.copy_from_slice(&bytes[112..144]);
        Ok(Self {
            observation,
            for_epoch,
            emission_udrt: value(72),
            before_digest,
            after_digest,
        })
    }
}

impl<'a> AdaptiveJournal<'a> {
    /// Explicit genesis initialization. Never overwrites an existing head or event.
    pub fn initialize(
        storage: &'a mut Storage,
        binding: [u8; 32],
        config: Config,
    ) -> Result<Self, JournalError> {
        {
            let guard = storage
                .lock_execution()
                .map_err(|_| JournalError::ExecutionLock)?;
            let prepared = prepare_initialize(storage, binding, config.clone())?;
            let mut batch = WriteBatch::default();
            prepared.append_checked(storage, &guard, &mut batch)?;
            write_sync(&storage.db, batch)?;
        }
        Ok(Self {
            storage,
            binding,
            config,
        })
    }

    /// Opens existing state only. Missing/corrupt data never becomes a new genesis.
    pub fn open(
        storage: &'a mut Storage,
        binding: [u8; 32],
        config: Config,
    ) -> Result<Self, JournalError> {
        config.validate()?;
        let journal = Self {
            storage,
            binding,
            config,
        };
        journal.read_head()?;
        Ok(journal)
    }

    fn read_head(&self) -> Result<(Controller, [u8; 32]), JournalError> {
        let (controller, digest, _) = read_checkpoint(self.storage, &self.binding, &self.config)?;
        Ok((controller, digest))
    }

    pub fn snapshot(&self) -> Result<Snapshot, JournalError> {
        Ok(self.read_head()?.0.snapshot())
    }

    /// Replays the complete journal from the supplied configuration. This checks
    /// internal consistency, not the authenticity of observations or genesis.
    /// Rejects histories beyond the caller's explicit work limit.
    pub fn verify_history(&self, max_records: u64) -> Result<u64, JournalError> {
        let (current, current_digest) = self.read_head()?;
        let expected_count = match current.snapshot().last_epoch {
            None => 0,
            Some(epoch) => epoch.checked_add(1).ok_or(JournalError::InvalidRecord)?,
        };
        if expected_count > max_records {
            return Err(JournalError::AuditLimit);
        }
        let mut replay = Controller::new(self.config.clone())?;
        let mut previous = encode_head(&self.binding, &replay).1;
        let mut count = 0;
        for entry in self
            .storage
            .db
            .iterator(IteratorMode::From(EVENT_PREFIX, Direction::Forward))
        {
            let (key, bytes) = entry?;
            if !key.starts_with(EVENT_PREFIX) {
                break;
            }
            if count >= expected_count || key.as_ref() != event_key(count).as_slice() {
                return Err(JournalError::InvalidRecord);
            }
            let record = RecordedCommand::decode(&bytes, &self.binding)?;
            if record.before_digest != previous {
                return Err(JournalError::InvalidRecord);
            }
            let command = replay.step(record.observation)?;
            previous = encode_head(&self.binding, &replay).1;
            if record.after_digest != previous
                || record.for_epoch != command.for_epoch
                || record.emission_udrt != command.emission_udrt
            {
                return Err(JournalError::InvalidRecord);
            }
            count += 1;
        }
        if count != expected_count || previous != current_digest || replay != current {
            return Err(JournalError::InvalidRecord);
        }
        Ok(count)
    }

    pub fn record(&self, epoch: u64) -> Result<Option<RecordedCommand>, JournalError> {
        read_record(self.storage, &self.binding, &self.config, epoch)
    }

    /// Commits the next command and its controller checkpoint in one synchronous
    /// RocksDB batch. An error can mean an unknown commit result. Read state before retry.
    pub fn advance(&mut self, observation: Observation) -> Result<Command, JournalError> {
        self.advance_with_commit(observation, write_sync)
    }

    fn advance_with_commit<F>(
        &mut self,
        observation: Observation,
        commit: F,
    ) -> Result<Command, JournalError>
    where
        F: FnOnce(&DB, WriteBatch) -> Result<(), JournalError>,
    {
        let guard = self
            .storage
            .lock_execution()
            .map_err(|_| JournalError::ExecutionLock)?;
        let prepared =
            prepare_transition(self.storage, self.binding, self.config.clone(), observation)?;
        let command = prepared
            .command()
            .cloned()
            .ok_or(JournalError::InvalidRecord)?;
        let mut batch = WriteBatch::default();
        prepared.append_checked(self.storage, &guard, &mut batch)?;
        commit(&self.storage.db, batch)?;
        Ok(command)
    }
}

/// A validated journal update that has not written any storage.
///
/// The update contains at most one bounded checkpoint and one fixed-size event.
/// Its private fields prevent replacing the command, writes or predecessor.
#[derive(Clone, Debug)]
pub struct PreparedJournalUpdate {
    expected_head: Option<Vec<u8>>,
    expected_previous_event: Option<(Vec<u8>, Vec<u8>)>,
    event_key: Option<Vec<u8>>,
    writes: Vec<(Vec<u8>, Vec<u8>)>,
    command: Option<Command>,
}
impl PreparedJournalUpdate {
    /// Exact immutable canonical writes for inspection or integration planning.
    /// Do not commit these bytes without append_checked and the lock contract below.
    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.writes
    }
    /// Exact bytes from the same read that decoded the predecessor controller.
    /// None means initialization requires an absent checkpoint and event prefix.
    pub fn expected_head(&self) -> Option<&[u8]> {
        self.expected_head.as_deref()
    }
    /// Initialization has no command. A transition proposes the following epoch.
    pub fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    /// Recheck the predecessor before appending any writes to a caller-owned batch.
    /// Failure leaves the supplied batch unchanged. This function does not commit.
    ///
    /// The caller MUST hold this storage handle's execution guard from this check
    /// through the final combined database write. MutexGuard's type does not prove
    /// storage identity; the caller must supply the guard from this same Storage.
    /// Do not place other writes to adaptive journal keys in the combined batch.
    /// All supported journal writers use the same execution lock. Raw DB writers
    /// must obey this contract too. A write error can have an unknown outcome;
    /// inspect the durable head before retrying. Reapplying a committed plan rejects.
    pub fn append_checked(
        &self,
        storage: &Storage,
        _guard: &std::sync::MutexGuard<'_, ()>,
        batch: &mut WriteBatch,
    ) -> Result<(), JournalError> {
        let actual = storage.db.get(HEAD_KEY)?;
        if actual.as_deref() != self.expected_head.as_deref() {
            return Err(JournalError::StalePreparation);
        }
        if self.expected_head.is_none() {
            ensure_uninitialized(storage)?;
        }
        if let Some((key, expected)) = &self.expected_previous_event {
            if storage.db.get(key)?.as_deref() != Some(expected.as_slice()) {
                return Err(JournalError::StalePreparation);
            }
        }
        if let Some(key) = &self.event_key {
            if storage.db.get(key)?.is_some() {
                return Err(JournalError::RecordConflict);
            }
        }
        // Every fallible read and validation precedes the first batch mutation.
        for (key, value) in &self.writes {
            batch.put(key, value);
        }
        Ok(())
    }
}

fn ensure_uninitialized(storage: &Storage) -> Result<(), JournalError> {
    if storage.db.get(HEAD_KEY)?.is_some() {
        return Err(JournalError::AlreadyInitialized);
    }
    if let Some(entry) = storage
        .db
        .iterator(IteratorMode::From(EVENT_PREFIX, Direction::Forward))
        .next()
    {
        let (key, _) = entry?;
        if key.starts_with(EVENT_PREFIX) {
            return Err(JournalError::AlreadyInitialized);
        }
    }
    Ok(())
}

/// Prepare an empty controller checkpoint without writing genesis or issuance.
/// No first-epoch command is inferred. append_checked must join the caller's batch.
pub fn prepare_initialize(
    storage: &Storage,
    binding: [u8; 32],
    config: Config,
) -> Result<PreparedJournalUpdate, JournalError> {
    let controller = Controller::new(config)?;
    ensure_uninitialized(storage)?;
    let (bytes, _) = encode_head(&binding, &controller);
    Ok(PreparedJournalUpdate {
        expected_head: None,
        expected_previous_event: None,
        event_key: None,
        writes: vec![(HEAD_KEY.to_vec(), bytes)],
        command: None,
    })
}

/// Prepare the next validated observation transition without writing storage.
/// The command does not mint or allocate tokens. No epoch timing is selected.
/// Concurrent supported writes are detected by append_checked's exact predecessor.
pub fn prepare_transition(
    storage: &Storage,
    binding: [u8; 32],
    config: Config,
    observation: Observation,
) -> Result<PreparedJournalUpdate, JournalError> {
    config.validate()?;
    let (mut next, before_digest, expected_head) = read_checkpoint(storage, &binding, &config)?;
    let expected_previous_event = match next.snapshot().last_epoch {
        Some(epoch) => {
            let key = event_key(epoch);
            let bytes = storage.db.get(&key)?.ok_or(JournalError::InvalidRecord)?;
            let prior = RecordedCommand::decode(&bytes, &binding)?;
            if prior.observation.epoch != epoch || prior.after_digest != before_digest {
                return Err(JournalError::InvalidRecord);
            }
            Some((key, bytes))
        }
        None => None,
    };
    let command = next.step(observation)?;
    let key = event_key(observation.epoch);
    if storage.db.get(&key)?.is_some() {
        return Err(JournalError::RecordConflict);
    }
    let (bytes, after_digest) = encode_head(&binding, &next);
    let record = RecordedCommand {
        observation,
        for_epoch: command.for_epoch,
        emission_udrt: command.emission_udrt,
        before_digest,
        after_digest,
    };
    Ok(PreparedJournalUpdate {
        expected_head: Some(expected_head),
        expected_previous_event,
        event_key: Some(key.clone()),
        writes: vec![(key, record.encode(&binding)), (HEAD_KEY.to_vec(), bytes)],
        command: Some(command),
    })
}

fn read_checkpoint(
    storage: &Storage,
    binding: &[u8; 32],
    config: &Config,
) -> Result<(Controller, [u8; 32], Vec<u8>), JournalError> {
    let bytes = storage
        .db
        .get(HEAD_KEY)?
        .ok_or(JournalError::MissingCheckpoint)?;
    let (controller, hash) = decode_checkpoint(&bytes, binding, config)?;
    let snapshot = controller.snapshot();
    if let Some(epoch) = snapshot.last_epoch {
        let record =
            read_record(storage, binding, config, epoch)?.ok_or(JournalError::InvalidRecord)?;
        if record.after_digest != hash
            || snapshot.errors_ppm.last().copied()
                != Some(config.target_ppm as i64 - record.observation.utilization_ppm as i64)
        {
            return Err(JournalError::InvalidRecord);
        }
    }
    Ok((controller, hash, bytes))
}

fn decode_checkpoint(
    bytes: &[u8],
    binding: &[u8; 32],
    config: &Config,
) -> Result<(Controller, [u8; 32]), JournalError> {
    if bytes.len() < 72 || bytes.len() > MAX_ENCODED_LEN + 72 || &bytes[..8] != HEAD_MAGIC {
        return Err(JournalError::InvalidCheckpoint);
    }
    let hash = digest(&bytes[..bytes.len() - 32]);
    if hash.as_slice() != &bytes[bytes.len() - 32..] {
        return Err(JournalError::InvalidCheckpoint);
    }
    if bytes[8..40] != *binding {
        return Err(JournalError::BindingMismatch);
    }
    let controller = Controller::decode(&bytes[40..bytes.len() - 32])?;
    let snapshot = controller.snapshot();
    if snapshot.config != *config {
        return Err(JournalError::ConfigMismatch);
    }
    Ok((controller, hash))
}

fn read_record(
    storage: &Storage,
    binding: &[u8; 32],
    config: &Config,
    epoch: u64,
) -> Result<Option<RecordedCommand>, JournalError> {
    let Some(bytes) = storage.db.get(event_key(epoch))? else {
        return Ok(None);
    };
    let record = RecordedCommand::decode(&bytes, binding)?;
    if record.observation.epoch != epoch
        || !(config.min_udrt..=config.max_udrt).contains(&record.emission_udrt)
    {
        return Err(JournalError::InvalidRecord);
    }
    Ok(Some(record))
}

/// A complete deterministic replay of the supplied immutable journal view.
/// Commands retain observation order. This result grants no issuance authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedJournal {
    pub snapshot: Snapshot,
    pub commands: Vec<RecordedCommand>,
}

/// Validate a caller-provided database view without reading or writing storage.
/// Unrelated prefixes are ignored. Unknown adaptive keys are rejected.
/// Count bounds are checked before decoding or replaying any command.
/// The caller must provide a coherent full view, including staged journal writes.
pub fn verify_view(
    values: &BTreeMap<Vec<u8>, Vec<u8>>,
    binding: [u8; 32],
    config: &Config,
    max_records: u64,
) -> Result<VerifiedJournal, JournalError> {
    config.validate()?;
    let mut count = 0u64;
    for (key, _) in values.range(b"adaptive:".to_vec()..) {
        if !key.starts_with(b"adaptive:") {
            break;
        }
        if key.as_slice() == HEAD_KEY {
            continue;
        }
        if !key.starts_with(EVENT_PREFIX) || key.len() != EVENT_PREFIX.len() + 8 {
            return Err(JournalError::InvalidRecord);
        }
        count = count.checked_add(1).ok_or(JournalError::AuditLimit)?;
        if count > max_records {
            return Err(JournalError::AuditLimit);
        }
    }
    let bytes = values
        .get(HEAD_KEY)
        .ok_or(JournalError::MissingCheckpoint)?;
    let (current, current_digest) = decode_checkpoint(bytes, &binding, config)?;
    let snapshot = current.snapshot();
    let expected_count = match snapshot.last_epoch {
        None => 0,
        Some(epoch) => epoch.checked_add(1).ok_or(JournalError::InvalidRecord)?,
    };
    if expected_count > max_records {
        return Err(JournalError::AuditLimit);
    }
    if count != expected_count {
        return Err(JournalError::InvalidRecord);
    }
    let mut replay = Controller::new(config.clone())?;
    let mut previous = encode_head(&binding, &replay).1;
    let mut commands = Vec::new();
    for epoch in 0..expected_count {
        let encoded = values
            .get(&event_key(epoch))
            .ok_or(JournalError::InvalidRecord)?;
        let record = RecordedCommand::decode(encoded, &binding)?;
        if record.observation.epoch != epoch || record.before_digest != previous {
            return Err(JournalError::InvalidRecord);
        }
        let command = replay.step(record.observation)?;
        previous = encode_head(&binding, &replay).1;
        if record.after_digest != previous
            || record.for_epoch != command.for_epoch
            || record.emission_udrt != command.emission_udrt
        {
            return Err(JournalError::InvalidRecord);
        }
        commands.push(record);
    }
    if previous != current_digest || replay != current {
        return Err(JournalError::InvalidRecord);
    }
    Ok(VerifiedJournal { snapshot, commands })
}

#[cfg(test)]
mod tests;
