//! Snapshot files (state sync v1, rule 3). At every `interval`th height the
//! application writes its committed database, except the state tree's own
//! records, as one sorted entry stream cut into chunks of at most 4 MiB, with
//! metadata that lists each chunk's SHA3-256. The bridge serves complete
//! snapshots from these files without calling the application; a joining node
//! checks every chunk and rebuilds the tree (C3).
//!
//! Layout under the configured directory:
//! `{height:020}/metadata.json` and `{height:020}/chunk-{index:06}`. A snapshot
//! is written under `.staging-{height:020}` from a RocksDB checkpoint at
//! `.checkpoint-{height:020}` and published by renaming the staging directory.
use crate::storage::state::Storage;
use anyhow::{ensure, Context, Result};
use rocksdb::IteratorMode;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Entry stream encoding, version 1.
pub const FORMAT: u32 = 1;
/// The largest chunk: CometBFT carries a chunk in one peer message.
pub const CHUNK_BYTES: usize = 4 << 20;
pub const METADATA_FILE: &str = "metadata.json";
/// Metadata read back from disk is bounded.
pub const MAX_METADATA_BYTES: u64 = 1 << 20;
/// Restore bounds. A snapshot is authenticated only once complete, so a
/// false one must not hold unbounded memory or disk before then: 256 GiB of
/// chunks, and no key or value above 64 MiB.
pub const MAX_CHUNKS: usize = 1 << 16;
pub const MAX_FIELD_BYTES: usize = 64 << 20;

/// Operator snapshot settings, a local node setting. The interval and the
/// count kept are E05 values; there are no defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotConfig {
    pub dir: PathBuf,
    pub interval: u64,
    pub keep: usize,
}
impl SnapshotConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.dir.is_absolute(),
            "Snapshot directory must be absolute"
        );
        ensure!(
            self.interval > 0 && self.keep > 0,
            "Snapshot interval and count kept must be positive"
        );
        Ok(())
    }
}

/// The committed head a snapshot holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub chain_id: String,
    pub height: u64,
    pub app_hash: String,
    pub state_digest: String,
    pub retained_from: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub format: u32,
    pub chain_id: String,
    pub height: u64,
    pub app_hash: String,
    pub state_digest: String,
    pub retained_from: u64,
    pub entries: u64,
    pub bytes: u64,
    /// SHA3-256 of each chunk, lowercase hex, in stream order.
    pub chunks: Vec<String>,
}
impl Metadata {
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
    /// The snapshot hash CometBFT carries: SHA3-256 of the metadata bytes.
    pub fn hash(&self) -> Result<[u8; 32]> {
        Ok(Sha3_256::digest(self.encode()?).into())
    }
    pub fn decode(raw: &[u8]) -> Result<Self> {
        let metadata: Self = serde_json::from_slice(raw)?;
        ensure!(metadata.encode()? == raw, "Noncanonical snapshot metadata");
        ensure!(
            metadata.format == FORMAT
                && !metadata.chunks.is_empty()
                && u32::try_from(metadata.chunks.len()).is_ok()
                && metadata.chunks.iter().all(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                }),
            "Invalid snapshot metadata"
        );
        Ok(metadata)
    }
}

pub fn snapshot_dir(root: &Path, height: u64) -> PathBuf {
    root.join(format!("{height:020}"))
}
pub fn chunk_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("chunk-{index:06}"))
}
fn staging_dir(root: &Path, height: u64) -> PathBuf {
    root.join(format!(".staging-{height:020}"))
}
fn checkpoint_dir(root: &Path, height: u64) -> PathBuf {
    root.join(format!(".checkpoint-{height:020}"))
}

/// Keys the snapshot leaves out: the state tree's records, which a joining
/// node rebuilds from the entries and checks against the state digest.
fn excluded(key: &[u8]) -> bool {
    key.starts_with(crate::state_tree::PREFIX)
}

/// One entry in the stream: key and value, each with a 32-bit big-endian
/// length.
pub fn encode_entry(key: &[u8], value: &[u8], out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&u32::try_from(key.len())?.to_be_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(&u32::try_from(value.len())?.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}

struct Chunks<'a> {
    dir: &'a Path,
    pending: Vec<u8>,
    hashes: Vec<String>,
    bytes: u64,
}
impl Chunks<'_> {
    fn push(&mut self, data: &[u8]) -> Result<()> {
        self.pending.extend_from_slice(data);
        while self.pending.len() >= CHUNK_BYTES {
            let rest = self.pending.split_off(CHUNK_BYTES);
            let full = std::mem::replace(&mut self.pending, rest);
            self.flush(&full)?;
        }
        Ok(())
    }
    fn flush(&mut self, chunk: &[u8]) -> Result<()> {
        let mut file = std::fs::File::create(chunk_path(self.dir, self.hashes.len()))?;
        file.write_all(chunk)?;
        file.sync_all()?;
        self.hashes.push(hex::encode(Sha3_256::digest(chunk)));
        self.bytes += u64::try_from(chunk.len())?;
        Ok(())
    }
    fn finish(mut self) -> Result<(Vec<String>, u64)> {
        if !self.pending.is_empty() {
            let last = std::mem::take(&mut self.pending);
            self.flush(&last)?;
        }
        Ok((self.hashes, self.bytes))
    }
}

/// Write the snapshot of the database at `source` (a checkpoint of the
/// committed database at `header.height`) under `root`, publish it and keep
/// the latest `keep` snapshots.
pub fn write(source: &Path, root: &Path, header: &Header, keep: usize) -> Result<Metadata> {
    let target = snapshot_dir(root, header.height);
    ensure!(!target.exists(), "Snapshot height already written");
    let staging = staging_dir(root, header.height);
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir(&staging)?;
    let storage = Storage::open_read_only(source.to_path_buf())?;
    let mut chunks = Chunks {
        dir: &staging,
        pending: Vec::with_capacity(CHUNK_BYTES),
        hashes: Vec::new(),
        bytes: 0,
    };
    let mut entries = 0u64;
    let mut entry = Vec::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (key, value) = item?;
        if excluded(&key) {
            continue;
        }
        entry.clear();
        encode_entry(&key, &value, &mut entry)?;
        chunks.push(&entry)?;
        entries += 1;
    }
    drop(storage);
    let (hashes, bytes) = chunks.finish()?;
    ensure!(!hashes.is_empty(), "Snapshot holds no entries");
    let metadata = Metadata {
        format: FORMAT,
        chain_id: header.chain_id.clone(),
        height: header.height,
        app_hash: header.app_hash.clone(),
        state_digest: header.state_digest.clone(),
        retained_from: header.retained_from,
        entries,
        bytes,
        chunks: hashes,
    };
    let mut file = std::fs::File::create(staging.join(METADATA_FILE))?;
    file.write_all(&metadata.encode()?)?;
    file.sync_all()?;
    std::fs::File::open(&staging)?.sync_all()?;
    std::fs::rename(&staging, &target)?;
    std::fs::File::open(root)?.sync_all()?;
    prune(root, keep)?;
    Ok(metadata)
}

/// Heights of the published snapshots under `root`, ascending.
pub fn published(root: &Path) -> Result<Vec<u64>> {
    let mut heights = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.len() == 20 && name.bytes().all(|b| b.is_ascii_digit()) {
            let height = name.parse()?;
            if snapshot_dir(root, height).join(METADATA_FILE).is_file() {
                heights.push(height);
            }
        }
    }
    heights.sort_unstable();
    Ok(heights)
}

/// Remove all but the latest `keep` published snapshots.
fn prune(root: &Path, keep: usize) -> Result<()> {
    let heights = published(root)?;
    for height in &heights[..heights.len().saturating_sub(keep)] {
        std::fs::remove_dir_all(snapshot_dir(root, *height))?;
    }
    Ok(())
}

/// Remove staging and checkpoint directories left by an interrupted writer.
fn remove_partial(root: &Path) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(".staging-") || name.starts_with(".checkpoint-") {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// Writes snapshots in the background: at a due height, commit takes a
/// RocksDB checkpoint (hard links, so the directory should be on the
/// database's filesystem) and a thread writes the files from it. A failure
/// never affects consensus; it is reported and the next due height retries.
pub struct SnapshotWriter {
    config: SnapshotConfig,
    job: Option<std::thread::JoinHandle<Result<Metadata>>>,
}
impl SnapshotWriter {
    pub fn new(config: SnapshotConfig) -> Result<Self> {
        config.validate()?;
        std::fs::create_dir_all(&config.dir)?;
        remove_partial(&config.dir)?;
        Ok(Self { config, job: None })
    }
    pub fn due(&self, height: u64) -> bool {
        height % self.config.interval == 0
    }
    /// Start the snapshot of `db` as committed at `header.height`. Call with
    /// the execution lock held, right after the commit write. A snapshot still
    /// being written makes this one skip.
    pub fn start(&mut self, db: &rocksdb::DB, header: Header) -> Result<()> {
        if self.job.as_ref().is_some_and(|job| !job.is_finished()) {
            return Ok(());
        }
        if let Err(error) = self.wait() {
            log::warn!("Snapshot writer failed: {error:#}");
        }
        remove_partial(&self.config.dir)?;
        let checkpoint = checkpoint_dir(&self.config.dir, header.height);
        rocksdb::checkpoint::Checkpoint::new(db)?
            .create_checkpoint(&checkpoint)
            .context("Snapshot checkpoint failed")?;
        let root = self.config.dir.clone();
        let keep = self.config.keep;
        self.job = Some(std::thread::spawn(move || {
            let result = write(&checkpoint, &root, &header, keep);
            let removed = std::fs::remove_dir_all(&checkpoint);
            let metadata = result?;
            removed?;
            Ok(metadata)
        }));
        Ok(())
    }
    /// Wait for the snapshot being written, if any, and return its metadata.
    pub fn wait(&mut self) -> Result<Option<Metadata>> {
        match self.job.take() {
            None => Ok(None),
            Some(job) => job
                .join()
                .map_err(|_| anyhow::anyhow!("Snapshot writer panicked"))?
                .map(Some),
        }
    }
}

/// The entries of a complete stream, in order.
pub fn decode_entries(mut stream: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut entries = Vec::new();
    let mut take = |stream: &mut &[u8]| -> Result<Vec<u8>> {
        ensure!(stream.len() >= 4, "Truncated snapshot entry");
        let (length, rest) = stream.split_at(4);
        let length = usize::try_from(u32::from_be_bytes(length.try_into()?))?;
        ensure!(rest.len() >= length, "Truncated snapshot entry");
        let (value, rest) = rest.split_at(length);
        *stream = rest;
        Ok(value.to_vec())
    };
    while !stream.is_empty() {
        let key = take(&mut stream)?;
        let value = take(&mut stream)?;
        entries.push((key, value));
    }
    Ok(entries)
}

/// What a chunk did to a restore in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// More chunks are needed.
    Accepted,
    /// The chunk does not match its listed hash: fetch it again from
    /// another sender.
    Refetch,
    /// Every chunk is in; the staging database is complete.
    Complete,
}

/// A snapshot being restored into a staging database (state sync v1,
/// rule 4). Chunks arrive in order; each must match its listed hash, and
/// the entries must be strictly ascending, outside the state tree, and add
/// up to the listed counts.
pub struct Restore {
    pub metadata: Metadata,
    staging: PathBuf,
    storage: Option<Storage>,
    next: usize,
    pending: Vec<u8>,
    last_key: Option<Vec<u8>>,
    entries: u64,
    bytes: u64,
}
impl Restore {
    /// Start restoring `metadata`, already matched to the trusted height and
    /// application hash, into a new database at `staging`.
    pub fn begin(metadata: Metadata, staging: PathBuf) -> Result<Self> {
        ensure!(
            metadata.chunks.len() <= MAX_CHUNKS,
            "Snapshot exceeds the restore bound"
        );
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        let storage = Storage::open(staging.clone())?;
        Ok(Self {
            metadata,
            staging,
            storage: Some(storage),
            next: 0,
            pending: Vec::new(),
            last_key: None,
            entries: 0,
            bytes: 0,
        })
    }
    pub fn staging(&self) -> &Path {
        &self.staging
    }
    pub fn storage(&self) -> Result<&Storage> {
        self.storage.as_ref().context("Restore storage closed")
    }
    /// Apply chunk `index`. An error means the snapshot is unusable.
    pub fn apply(&mut self, index: usize, chunk: &[u8]) -> Result<Applied> {
        ensure!(index == self.next, "Snapshot chunk out of order");
        let expected = self
            .metadata
            .chunks
            .get(index)
            .context("Snapshot chunk index outside the snapshot")?;
        if chunk.len() > CHUNK_BYTES || hex::encode(Sha3_256::digest(chunk)) != *expected {
            return Ok(Applied::Refetch);
        }
        self.pending.extend_from_slice(chunk);
        let mut batch = rocksdb::WriteBatch::default();
        let mut offset = 0;
        while let Some((key, value, used)) = next_entry(&self.pending[offset..])? {
            ensure!(
                self.last_key.as_deref().is_none_or(|last| last < key) && !excluded(key),
                "Snapshot entries out of order or inside the state tree"
            );
            batch.put(key, value);
            self.last_key = Some(key.to_vec());
            self.entries += 1;
            offset += used;
        }
        self.pending.drain(..offset);
        self.storage()?.db.write(batch)?;
        self.bytes += u64::try_from(chunk.len())?;
        self.next += 1;
        if self.next < self.metadata.chunks.len() {
            return Ok(Applied::Accepted);
        }
        ensure!(
            self.pending.is_empty()
                && self.entries == self.metadata.entries
                && self.bytes == self.metadata.bytes,
            "Snapshot stream differs from its metadata"
        );
        Ok(Applied::Complete)
    }
    /// Close the staging database and return its path.
    pub fn close(mut self) -> PathBuf {
        self.storage = None;
        self.staging.clone()
    }
    /// Discard the staging database.
    pub fn discard(mut self) -> Result<()> {
        self.storage = None;
        if self.staging.exists() {
            std::fs::remove_dir_all(&self.staging)?;
        }
        Ok(())
    }
}

/// The first complete entry of `stream` and the bytes it used, or None when
/// the stream holds only part of one.
fn next_entry(stream: &[u8]) -> Result<Option<(&[u8], &[u8], usize)>> {
    let field = |at: usize| -> Result<Option<(usize, usize)>> {
        let Some(length) = stream.get(at..at + 4) else {
            return Ok(None);
        };
        let length = usize::try_from(u32::from_be_bytes(length.try_into()?))?;
        ensure!(
            length <= MAX_FIELD_BYTES,
            "Snapshot entry exceeds the restore bound"
        );
        Ok((stream.len() >= at + 4 + length).then_some((at + 4, length)))
    };
    let Some((key_at, key_len)) = field(0)? else {
        return Ok(None);
    };
    let Some((value_at, value_len)) = field(key_at + key_len)? else {
        return Ok(None);
    };
    Ok(Some((
        &stream[key_at..key_at + key_len],
        &stream[value_at..value_at + value_len],
        value_at + value_len,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(height: u64) -> Header {
        Header {
            chain_id: "snapshot-test".into(),
            height,
            app_hash: "a".repeat(64),
            state_digest: "b".repeat(64),
            retained_from: 1,
        }
    }

    /// An entry larger than a chunk spans chunks; the stream reads back as
    /// the database without the state tree's records.
    #[test]
    fn stream_spans_chunks_and_leaves_out_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let storage = Storage::open(db.clone()).unwrap();
        let large = vec![7u8; CHUNK_BYTES + 100];
        storage.db.put(b"acct:large", &large).unwrap();
        storage.db.put(b"acct:small", b"1").unwrap();
        storage.db.put(b"merkle:node", b"tree").unwrap();
        drop(storage);
        let root = dir.path().join("snapshots");
        std::fs::create_dir(&root).unwrap();
        let metadata = write(&db, &root, &header(4), 2).unwrap();
        assert_eq!(metadata.chunks.len(), 2);
        assert_eq!(metadata.entries, 2);
        let target = snapshot_dir(&root, 4);
        let mut stream = Vec::new();
        for (index, hash) in metadata.chunks.iter().enumerate() {
            let chunk = std::fs::read(chunk_path(&target, index)).unwrap();
            assert!(chunk.len() <= CHUNK_BYTES);
            assert_eq!(&hex::encode(Sha3_256::digest(&chunk)), hash);
            stream.extend(chunk);
        }
        assert_eq!(u64::try_from(stream.len()).unwrap(), metadata.bytes);
        assert_eq!(
            decode_entries(&stream).unwrap(),
            vec![
                (b"acct:large".to_vec(), large),
                (b"acct:small".to_vec(), b"1".to_vec())
            ]
        );
        let raw = std::fs::read(target.join(METADATA_FILE)).unwrap();
        assert_eq!(Metadata::decode(&raw).unwrap(), metadata);
        assert_eq!(
            metadata.hash().unwrap(),
            <[u8; 32]>::from(Sha3_256::digest(&raw))
        );
    }

    /// Only the latest `keep` snapshots stay, and a partial directory left by
    /// an interrupted writer is removed.
    #[test]
    fn writer_keeps_the_latest_snapshots_and_removes_partial_ones() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let storage = Storage::open(db.clone()).unwrap();
        storage.db.put(b"acct:a", b"1").unwrap();
        drop(storage);
        let root = dir.path().join("snapshots");
        std::fs::create_dir(&root).unwrap();
        for height in [2, 4, 6] {
            write(&db, &root, &header(height), 2).unwrap();
        }
        assert_eq!(published(&root).unwrap(), [4, 6]);
        assert!(write(&db, &root, &header(6), 2).is_err());
        std::fs::create_dir(root.join(".staging-00000000000000000008")).unwrap();
        std::fs::create_dir(root.join(".checkpoint-00000000000000000008")).unwrap();
        SnapshotWriter::new(SnapshotConfig {
            dir: root.clone(),
            interval: 2,
            keep: 2,
        })
        .unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["00000000000000000004", "00000000000000000006"]);
    }

    #[test]
    fn metadata_and_config_are_checked() {
        let metadata = Metadata {
            format: FORMAT,
            chain_id: "c".into(),
            height: 1,
            app_hash: "a".repeat(64),
            state_digest: "b".repeat(64),
            retained_from: 1,
            entries: 1,
            bytes: 9,
            chunks: vec!["c".repeat(64)],
        };
        let raw = metadata.encode().unwrap();
        assert_eq!(Metadata::decode(&raw).unwrap(), metadata);
        for invalid in [
            Metadata {
                format: 2,
                ..metadata.clone()
            },
            Metadata {
                chunks: vec![],
                ..metadata.clone()
            },
            Metadata {
                chunks: vec!["C".repeat(64)],
                ..metadata.clone()
            },
        ] {
            assert!(Metadata::decode(&invalid.encode().unwrap()).is_err());
        }
        let mut spaced = raw.clone();
        spaced.insert(1, b' ');
        assert!(Metadata::decode(&spaced).is_err());
        assert!(decode_entries(&[0, 0, 0, 5, 1]).is_err());
        for config in [
            SnapshotConfig {
                dir: "relative".into(),
                interval: 1,
                keep: 1,
            },
            SnapshotConfig {
                dir: "/abs".into(),
                interval: 0,
                keep: 1,
            },
            SnapshotConfig {
                dir: "/abs".into(),
                interval: 1,
                keep: 0,
            },
        ] {
            assert!(config.validate().is_err());
        }
    }
}
