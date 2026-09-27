//! Recovery accounts and origins read from committed storage on first use
//! (account model v2, step B1b-2).
//!
//! A complete set holds every value in memory; genesis, tests and the complete
//! history check use it. A staged set holds only the values changed in the
//! current block (its overlay). Any other value is read from committed storage
//! on first use, adjusted to the set's height and cached. The cache is not
//! state: equality, encoding and the block diff see only the overlay and
//! committed storage, so per-block work follows the entries a block touches.
use crate::storage::state::Storage;
use anyhow::{anyhow, ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

/// A value stored under `PREFIX` followed by its ID, as canonical JSON.
pub trait Stored: Clone + PartialEq + fmt::Debug + Serialize + DeserializeOwned {
    const PREFIX: &'static str;
    const WHAT: &'static str;
    /// The committed value as seen at `height`. It may differ from the stored
    /// value only in fields that are never written on their own.
    fn at_height(self, height: u64) -> Result<Self>;
}

#[derive(Clone)]
struct Base {
    storage: Arc<Storage>,
    height: u64,
}

pub struct StoredSet<V> {
    base: Option<Base>,
    overlay: BTreeMap<String, Arc<V>>,
    cache: Mutex<BTreeMap<String, Arc<V>>>,
}

#[cfg(test)]
thread_local! {
    /// Committed entries read by staged sets on this thread.
    pub(crate) static STORED_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Complete recovery book loads on this thread.
    pub(crate) static COMPLETE_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<V: Stored> StoredSet<V> {
    pub(crate) fn complete(values: BTreeMap<String, V>) -> Self {
        Self {
            base: None,
            overlay: values.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            cache: Mutex::default(),
        }
    }
    /// Committed values at `height`, read on first use.
    pub(crate) fn staged(storage: Arc<Storage>, height: u64) -> Self {
        Self {
            base: Some(Base { storage, height }),
            overlay: BTreeMap::new(),
            cache: Mutex::default(),
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.base.is_none()
    }
    /// For `skip_serializing_if`: a complete set with no values.
    pub(crate) fn is_empty_complete(&self) -> bool {
        self.base.is_none() && self.overlay.is_empty()
    }
    fn read(base: &Base, id: &str) -> Result<Option<V>> {
        #[cfg(test)]
        STORED_READS.with(|n| n.set(n.get() + 1));
        base.storage
            .db
            .get(format!("{}{id}", V::PREFIX))?
            .map(|raw| Self::decode(&raw))
            .transpose()
    }
    fn decode(raw: &[u8]) -> Result<V> {
        let value: V =
            serde_json::from_slice(raw).with_context(|| format!("Invalid recovery {}", V::WHAT))?;
        ensure!(
            serde_json::to_vec(&value)? == raw,
            "Recovery {} encoding is not canonical",
            V::WHAT
        );
        Ok(value)
    }
    /// The value for `id`: the overlay's, else (staged) the committed value at
    /// this set's height, read once and cached.
    pub(crate) fn find(&self, id: &str) -> Result<Option<Arc<V>>> {
        if let Some(value) = self.overlay.get(id) {
            return Ok(Some(value.clone()));
        }
        let Some(base) = &self.base else {
            return Ok(None);
        };
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow!("Recovery {} cache lock poisoned", V::WHAT))?;
        if let Some(value) = cache.get(id) {
            return Ok(Some(value.clone()));
        }
        let Some(value) = Self::read(base, id)? else {
            return Ok(None);
        };
        let value = Arc::new(value.at_height(base.height)?);
        cache.insert(id.to_owned(), value.clone());
        Ok(Some(value))
    }
    pub(crate) fn contains(&self, id: &str) -> Result<bool> {
        Ok(self.find(id)?.is_some())
    }
    fn cache_mut(&mut self) -> &mut BTreeMap<String, Arc<V>> {
        // A poisoned cache holds only committed values; recover it.
        self.cache.get_mut().unwrap_or_else(|e| e.into_inner())
    }
    /// Set `id` in the overlay.
    pub(crate) fn insert(&mut self, id: String, value: V) {
        self.cache_mut().remove(&id);
        self.overlay.insert(id, Arc::new(value));
    }
    /// Mutable access through the overlay, copying a committed value into it.
    pub(crate) fn find_mut(&mut self, id: &str) -> Result<Option<&mut V>> {
        if !self.overlay.contains_key(id) {
            let Some(value) = self.find(id)? else {
                return Ok(None);
            };
            self.cache_mut().remove(id);
            self.overlay.insert(id.to_owned(), value);
        }
        Ok(self.overlay.get_mut(id).map(Arc::make_mut))
    }
    /// The overlay entry for `id`, for a checkpoint.
    pub(crate) fn overlay_entry(&self, id: &str) -> Option<Arc<V>> {
        self.overlay.get(id).cloned()
    }
    /// Restore an overlay entry captured by `overlay_entry`. Without one, a
    /// staged set shows the committed value again.
    pub(crate) fn restore(&mut self, id: String, entry: Option<Arc<V>>) {
        self.cache_mut().remove(&id);
        match entry {
            Some(value) => self.overlay.insert(id, value),
            None => self.overlay.remove(&id),
        };
    }
    /// Changed values (staged) or every value (complete).
    pub(crate) fn overlay(&self) -> &BTreeMap<String, Arc<V>> {
        &self.overlay
    }
    /// Mutable overlay values: every value of a complete set.
    pub(crate) fn overlay_values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.overlay.values_mut().map(Arc::make_mut)
    }
    /// Every value. Only a complete set holds them; reading every committed
    /// entry is the complete check's job, so a staged set refuses.
    pub(crate) fn all(&self) -> Result<&BTreeMap<String, Arc<V>>> {
        ensure!(
            self.base.is_none(),
            "Staged recovery {} set cannot list every entry",
            V::WHAT
        );
        Ok(&self.overlay)
    }
    /// The stored committed value, not adjusted to the set's height, for the
    /// block diff. A complete set has no committed storage.
    pub(crate) fn committed(&self, id: &str) -> Result<Option<V>> {
        match &self.base {
            Some(base) => Self::read(base, id),
            None => Ok(None),
        }
    }
    /// This staged set at a later height. Committed values are adjusted when
    /// read; the caller adjusts overlay values.
    pub(crate) fn staged_at(&self, height: u64) -> Result<Self> {
        let base = self
            .base
            .as_ref()
            .context("Complete recovery set has no committed height")?;
        Ok(Self {
            base: Some(Base {
                storage: base.storage.clone(),
                height,
            }),
            overlay: self.overlay.clone(),
            cache: Mutex::default(),
        })
    }
    /// Every value, scanning committed storage for a staged set. Test oracles
    /// only; production code never scans.
    #[cfg(test)]
    pub(crate) fn materialized(&self) -> Result<BTreeMap<String, Arc<V>>> {
        let mut values = BTreeMap::new();
        if let Some(base) = &self.base {
            let prefix = V::PREFIX.as_bytes();
            for item in base.storage.db.iterator(rocksdb::IteratorMode::From(
                prefix,
                rocksdb::Direction::Forward,
            )) {
                let (key, raw) = item?;
                let Some(id) = key.strip_prefix(prefix) else {
                    break;
                };
                let id = std::str::from_utf8(id)?.to_owned();
                if self.overlay.contains_key(&id) {
                    continue;
                }
                let value = Self::decode(&raw)?.at_height(base.height)?;
                values.insert(id, Arc::new(value));
            }
        }
        values.extend(self.overlay.clone());
        Ok(values)
    }
}
impl<V: Stored> FromIterator<(String, V)> for StoredSet<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(values: I) -> Self {
        Self::complete(values.into_iter().collect())
    }
}
impl<V: Stored> Default for StoredSet<V> {
    fn default() -> Self {
        Self::complete(BTreeMap::new())
    }
}
impl<V: Clone> Clone for StoredSet<V> {
    fn clone(&self) -> Self {
        let cache = self.cache.lock().map(|c| c.clone()).unwrap_or_default();
        Self {
            base: self.base.clone(),
            overlay: self.overlay.clone(),
            cache: Mutex::new(cache),
        }
    }
}
/// Equal state: the same committed base and the same overlay. Cached reads
/// are not state.
impl<V: PartialEq> PartialEq for StoredSet<V> {
    fn eq(&self, other: &Self) -> bool {
        self.overlay == other.overlay
            && match (&self.base, &other.base) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(&a.storage, &b.storage) && a.height == b.height,
                _ => false,
            }
    }
}
impl<V: Eq> Eq for StoredSet<V> {}
impl<V: fmt::Debug> fmt::Debug for StoredSet<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredSet")
            .field("height", &self.base.as_ref().map(|b| b.height))
            .field("overlay", &self.overlay)
            .finish()
    }
}
/// Only a complete set has an encoding: the map of every value.
impl<V: Serialize> Serialize for StoredSet<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        if self.base.is_some() {
            return Err(serde::ser::Error::custom(
                "staged recovery set has no complete encoding",
            ));
        }
        serializer.collect_map(self.overlay.iter().map(|(k, v)| (k, &**v)))
    }
}
impl<'de, V: Stored> Deserialize<'de> for StoredSet<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Ok(Self::complete(BTreeMap::deserialize(deserializer)?))
    }
}

/// Map-like access to a complete set, for tests.
#[cfg(test)]
impl<V: Stored> StoredSet<V> {
    fn test_values(&self) -> &BTreeMap<String, Arc<V>> {
        assert!(
            self.base.is_none(),
            "test accessor on a staged recovery set"
        );
        &self.overlay
    }
    pub(crate) fn get(&self, id: &str) -> Option<&V> {
        self.test_values().get(id).map(|v| &**v)
    }
    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut V> {
        self.test_values();
        self.overlay.get_mut(id).map(Arc::make_mut)
    }
    pub(crate) fn contains_key(&self, id: &str) -> bool {
        self.test_values().contains_key(id)
    }
    pub(crate) fn len(&self) -> usize {
        self.test_values().len()
    }
    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.test_values().keys()
    }
    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.test_values().values().map(|v| &**v)
    }
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.test_values();
        self.overlay_values_mut()
    }
    pub(crate) fn remove(&mut self, id: &str) -> Option<V> {
        self.test_values();
        self.overlay.remove(id).map(|v| (*v).clone())
    }
    pub(crate) fn pop_first(&mut self) -> Option<(String, V)> {
        self.test_values();
        self.overlay.pop_first().map(|(k, v)| (k, (*v).clone()))
    }
}
#[cfg(test)]
impl<V: Stored> std::ops::Index<&str> for StoredSet<V> {
    type Output = V;
    fn index(&self, id: &str) -> &V {
        self.get(id).expect("recovery entry present")
    }
}
#[cfg(test)]
impl<V: Stored> std::ops::Index<&String> for StoredSet<V> {
    type Output = V;
    fn index(&self, id: &String) -> &V {
        self.get(id).expect("recovery entry present")
    }
}
