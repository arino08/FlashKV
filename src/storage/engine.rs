//! Thread-Safe Storage Engine with Expiry Support
//!
//! This module implements the core storage engine for FlashKV.
//! It provides a thread-safe, concurrent HashMap with TTL (Time-To-Live) support.
//! It also supports List data structures (similar to Redis lists).
//!
//! ## Design Decisions
//!
//! 1. **Sharded Locks**: Instead of one big lock, we use multiple shards to reduce contention.
//! 2. **Lazy Expiry**: Keys are checked for expiry on access (lazy) plus background cleanup.
//! 3. **Arc<RwLock>**: Allows multiple concurrent readers with exclusive writers.
//! 4. **Separate List Storage**: Lists are stored separately from strings for type safety.
//!
//! ## Concurrency Model
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     StorageEngine                           │
//! │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │
//! │  │ Shard 0 │ │ Shard 1 │ │ Shard 2 │ │ Shard N │           │
//! │  │ RwLock  │ │ RwLock  │ │ RwLock  │ │ RwLock  │           │
//! │  │ HashMap │ │ HashMap │ │ HashMap │ │ HashMap │           │
//! │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! Keys are distributed across shards using a hash function.
//! This allows multiple threads to read/write different keys concurrently.

use bytes::Bytes;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Number of shards for the storage engine.
/// More shards = less lock contention, but more memory overhead.
/// 64 is a good balance for most workloads.
const NUM_SHARDS: usize = 64;

/// Represents a stored value with optional expiry time.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The actual value stored
    pub value: Bytes,
    /// When this entry expires (None = never expires)
    pub expires_at: Option<Instant>,
    /// When this entry was created
    pub created_at: Instant,
    /// Last access time (for potential LRU eviction in the future)
    pub last_accessed: Instant,
}

impl Entry {
    /// Creates a new entry without expiry.
    pub fn new(value: Bytes) -> Self {
        let now = Instant::now();
        Self {
            value,
            expires_at: None,
            created_at: now,
            last_accessed: now,
        }
    }

    /// Creates a new entry with TTL.
    pub fn with_ttl(value: Bytes, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            value,
            expires_at: Some(now + ttl),
            created_at: now,
            last_accessed: now,
        }
    }

    /// Checks if this entry has expired.
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() >= exp)
            .unwrap_or(false)
    }

    /// Returns the remaining TTL in milliseconds, or None if no expiry.
    pub fn ttl_ms(&self) -> Option<u64> {
        self.expires_at.map(|exp| {
            let now = Instant::now();
            if now >= exp {
                0
            } else {
                (exp - now).as_millis() as u64
            }
        })
    }
}

/// Represents a stored list with optional expiry time.
#[derive(Debug, Clone)]
pub struct ListEntry {
    /// The actual list data stored as a deque for O(1) push/pop on both ends
    pub data: VecDeque<Bytes>,
    /// When this entry expires (None = never expires)
    pub expires_at: Option<Instant>,
    /// When this entry was created
    pub created_at: Instant,
}

impl ListEntry {
    /// Creates a new empty list entry without expiry.
    pub fn new() -> Self {
        Self {
            data: VecDeque::new(),
            expires_at: None,
            created_at: Instant::now(),
        }
    }

    /// Checks if this list entry has expired.
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() >= exp)
            .unwrap_or(false)
    }
}

impl Default for ListEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// A single shard containing a portion of the key-value pairs.
#[derive(Debug)]
struct Shard {
    /// The actual data storage for strings
    data: RwLock<HashMap<Bytes, Entry>>,
    /// The actual data storage for lists
    lists: RwLock<HashMap<Bytes, ListEntry>>,
}

impl Shard {
    fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            lists: RwLock::new(HashMap::new()),
        }
    }
}

/// The main storage engine for FlashKV.
///
/// This is the "brain" of the database - it stores all key-value pairs
/// and handles concurrent access from multiple client connections.
///
/// # Thread Safety
///
/// This struct is designed to be wrapped in an `Arc` and shared across
/// all client handler tasks. All operations are thread-safe.
///
/// # Example
///
/// ```
/// use flashkv::storage::StorageEngine;
/// use bytes::Bytes;
/// use std::time::Duration;
///
/// let engine = StorageEngine::new();
///
/// // Set a key
/// engine.set(Bytes::from("name"), Bytes::from("Ariz"));
///
/// // Get the value
/// let value = engine.get(&Bytes::from("name"));
/// assert_eq!(value, Some(Bytes::from("Ariz")));
///
/// // Set with expiry
/// engine.set_with_ttl(Bytes::from("session"), Bytes::from("abc123"), Duration::from_secs(60));
/// ```
pub struct StorageEngine {
    /// Sharded storage for reduced lock contention
    shards: Vec<Shard>,

    /// Statistics: total number of keys (approximate)
    key_count: AtomicU64,

    /// Statistics: total GET operations
    get_count: AtomicU64,

    /// Statistics: total SET operations
    set_count: AtomicU64,

    /// Statistics: total DEL operations
    del_count: AtomicU64,

    /// Statistics: number of expired keys cleaned up
    expired_count: AtomicU64,

    /// Statistics: total list operations
    list_op_count: AtomicU64,
}

impl std::fmt::Debug for StorageEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageEngine")
            .field("shards", &self.shards.len())
            .field("key_count", &self.key_count.load(Ordering::Relaxed))
            .field("get_count", &self.get_count.load(Ordering::Relaxed))
            .field("set_count", &self.set_count.load(Ordering::Relaxed))
            .finish()
    }
}

impl Default for StorageEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageEngine {
    /// Creates a new storage engine with default settings.
    pub fn new() -> Self {
        let shards = (0..NUM_SHARDS).map(|_| Shard::new()).collect();

        Self {
            shards,
            key_count: AtomicU64::new(0),
            get_count: AtomicU64::new(0),
            set_count: AtomicU64::new(0),
            del_count: AtomicU64::new(0),
            expired_count: AtomicU64::new(0),
            list_op_count: AtomicU64::new(0),
        }
    }

    /// Determines which shard a key belongs to.
    #[inline]
    fn shard_index(&self, key: &[u8]) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % NUM_SHARDS
    }

    /// Gets the shard for a given key.
    #[inline]
    fn get_shard(&self, key: &[u8]) -> &Shard {
        &self.shards[self.shard_index(key)]
    }

    /// Sets a key-value pair without expiry.
    ///
    /// If the key already exists, its value is overwritten.
    ///
    /// # Returns
    ///
    /// Returns `true` if a new key was created, `false` if an existing key was updated.
    pub fn set(&self, key: Bytes, value: Bytes) -> bool {
        self.set_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(&key);
        let mut data = shard.data.write().unwrap();

        let is_new = !data.contains_key(&key);
        data.insert(key, Entry::new(value));

        if is_new {
            self.key_count.fetch_add(1, Ordering::Relaxed);
        }

        is_new
    }

    /// Sets a key-value pair with a TTL (Time-To-Live).
    ///
    /// The key will automatically expire after the specified duration.
    ///
    /// # Returns
    ///
    /// Returns `true` if a new key was created, `false` if an existing key was updated.
    pub fn set_with_ttl(&self, key: Bytes, value: Bytes, ttl: Duration) -> bool {
        self.set_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(&key);
        let mut data = shard.data.write().unwrap();

        let is_new = !data.contains_key(&key);
        data.insert(key, Entry::with_ttl(value, ttl));

        if is_new {
            self.key_count.fetch_add(1, Ordering::Relaxed);
        }

        is_new
    }

    /// Gets the value for a key.
    ///
    /// Returns `None` if the key doesn't exist or has expired.
    /// This implements "lazy expiry" - expired keys are detected and removed on access.
    pub fn get(&self, key: &Bytes) -> Option<Bytes> {
        self.get_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);

        // First, try a read lock (fast path for existing, non-expired keys)
        {
            let data = shard.data.read().unwrap();
            if let Some(entry) = data.get(key) {
                if !entry.is_expired() {
                    return Some(entry.value.clone());
                }
            } else {
                return None;
            }
        }

        // Key exists but is expired - need write lock to remove it
        let mut data = shard.data.write().unwrap();
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                self.key_count.fetch_sub(1, Ordering::Relaxed);
                self.expired_count.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            // Race: another thread may have updated the key
            return Some(entry.value.clone());
        }

        None
    }

    /// Gets the full entry for a key (including metadata).
    ///
    /// This is useful for commands like TTL that need access to expiry information.
    pub fn get_entry(&self, key: &Bytes) -> Option<Entry> {
        let shard = self.get_shard(key);

        {
            let data = shard.data.read().unwrap();
            if let Some(entry) = data.get(key) {
                if !entry.is_expired() {
                    return Some(entry.clone());
                }
            } else {
                return None;
            }
        }

        // Lazy cleanup of expired key
        let mut data = shard.data.write().unwrap();
        if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                data.remove(key);
                self.key_count.fetch_sub(1, Ordering::Relaxed);
                self.expired_count.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            return Some(entry.clone());
        }

        None
    }

    /// Deletes a key from the database.
    ///
    /// # Returns
    ///
    /// Returns `true` if the key was deleted, `false` if it didn't exist.
    pub fn delete(&self, key: &Bytes) -> bool {
        self.del_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);
        let mut data = shard.data.write().unwrap();

        if data.remove(key).is_some() {
            self.key_count.fetch_sub(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Deletes multiple keys from the database.
    ///
    /// # Returns
    ///
    /// Returns the number of keys that were deleted.
    pub fn delete_many(&self, keys: &[Bytes]) -> u64 {
        let mut deleted = 0;
        for key in keys {
            if self.delete(key) {
                deleted += 1;
            }
        }
        deleted
    }

    /// Checks if a key exists (and is not expired).
    pub fn exists(&self, key: &Bytes) -> bool {
        let shard = self.get_shard(key);
        let data = shard.data.read().unwrap();

        data.get(key).map(|e| !e.is_expired()).unwrap_or(false)
    }

    /// Counts how many of the given keys exist.
    pub fn exists_many(&self, keys: &[Bytes]) -> u64 {
        keys.iter().filter(|k| self.exists(k)).count() as u64
    }

    /// Sets an expiry time on an existing key.
    ///
    /// # Returns
    ///
    /// Returns `true` if the expiry was set, `false` if the key doesn't exist.
    pub fn expire(&self, key: &Bytes, ttl: Duration) -> bool {
        let shard = self.get_shard(key);
        let mut data = shard.data.write().unwrap();

        if let Some(entry) = data.get_mut(key) {
            if entry.is_expired() {
                data.remove(key);
                self.key_count.fetch_sub(1, Ordering::Relaxed);
                self.expired_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            entry.expires_at = Some(Instant::now() + ttl);
            true
        } else {
            false
        }
    }

    /// Removes the expiry from a key (makes it persistent).
    ///
    /// # Returns
    ///
    /// Returns `true` if the expiry was removed, `false` if the key doesn't exist
    /// or didn't have an expiry.
    pub fn persist(&self, key: &Bytes) -> bool {
        let shard = self.get_shard(key);
        let mut data = shard.data.write().unwrap();

        if let Some(entry) = data.get_mut(key) {
            if entry.is_expired() {
                data.remove(key);
                self.key_count.fetch_sub(1, Ordering::Relaxed);
                self.expired_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            if entry.expires_at.is_some() {
                entry.expires_at = None;
                return true;
            }
        }
        false
    }

    /// Gets the remaining TTL for a key in seconds.
    ///
    /// # Returns
    ///
    /// - `Some(seconds)` if the key exists and has an expiry
    /// - `Some(-1)` if the key exists but has no expiry
    /// - `None` if the key doesn't exist
    pub fn ttl(&self, key: &Bytes) -> Option<i64> {
        self.get_entry(key).map(|entry| {
            entry
                .expires_at
                .map(|exp| {
                    let now = Instant::now();
                    if now >= exp {
                        0
                    } else {
                        (exp - now).as_secs() as i64
                    }
                })
                .unwrap_or(-1)
        })
    }

    /// Gets the remaining TTL for a key in milliseconds.
    pub fn pttl(&self, key: &Bytes) -> Option<i64> {
        self.get_entry(key).map(|entry| {
            entry
                .expires_at
                .map(|exp| {
                    let now = Instant::now();
                    if now >= exp {
                        0
                    } else {
                        (exp - now).as_millis() as i64
                    }
                })
                .unwrap_or(-1)
        })
    }

    /// Increments an integer value by 1.
    ///
    /// If the key doesn't exist, it's set to 0 before the operation.
    /// Returns an error if the value is not a valid integer.
    pub fn incr(&self, key: &Bytes) -> Result<i64, &'static str> {
        self.incr_by(key, 1)
    }

    /// Increments an integer value by a specified amount.
    pub fn incr_by(&self, key: &Bytes, delta: i64) -> Result<i64, &'static str> {
        let shard = self.get_shard(key);
        let mut data = shard.data.write().unwrap();

        let current = if let Some(entry) = data.get(key) {
            if entry.is_expired() {
                0
            } else {
                let s = std::str::from_utf8(&entry.value)
                    .map_err(|_| "value is not an integer or out of range")?;
                s.parse::<i64>()
                    .map_err(|_| "value is not an integer or out of range")?
            }
        } else {
            0
        };

        let new_value = current
            .checked_add(delta)
            .ok_or("increment would overflow")?;

        let value_bytes = Bytes::from(new_value.to_string());

        // Preserve TTL if the key existed
        let expires_at = data
            .get(key)
            .and_then(|e| if e.is_expired() { None } else { e.expires_at });

        let is_new = !data.contains_key(key);
        let now = Instant::now();
        data.insert(
            key.clone(),
            Entry {
                value: value_bytes,
                expires_at,
                created_at: now,
                last_accessed: now,
            },
        );

        if is_new {
            self.key_count.fetch_add(1, Ordering::Relaxed);
        }

        Ok(new_value)
    }

    /// Decrements an integer value by 1.
    pub fn decr(&self, key: &Bytes) -> Result<i64, &'static str> {
        self.incr_by(key, -1)
    }

    /// Decrements an integer value by a specified amount.
    pub fn decr_by(&self, key: &Bytes, delta: i64) -> Result<i64, &'static str> {
        self.incr_by(key, -delta)
    }

    /// Appends a value to an existing string.
    ///
    /// If the key doesn't exist, it's created with the given value.
    ///
    /// # Returns
    ///
    /// Returns the length of the string after the append.
    pub fn append(&self, key: &Bytes, value: &Bytes) -> usize {
        let shard = self.get_shard(key);
        let mut data = shard.data.write().unwrap();

        if let Some(entry) = data.get_mut(key) {
            if entry.is_expired() {
                // Treat as new key
                let new_entry = Entry::new(value.clone());
                let len = value.len();
                data.insert(key.clone(), new_entry);
                return len;
            }

            // Append to existing value
            let mut new_value = Vec::with_capacity(entry.value.len() + value.len());
            new_value.extend_from_slice(&entry.value);
            new_value.extend_from_slice(value);
            let len = new_value.len();
            entry.value = Bytes::from(new_value);
            entry.last_accessed = Instant::now();
            len
        } else {
            // Create new key
            self.key_count.fetch_add(1, Ordering::Relaxed);
            let len = value.len();
            data.insert(key.clone(), Entry::new(value.clone()));
            len
        }
    }

    /// Gets the length of a string value.
    ///
    /// Returns 0 if the key doesn't exist.
    pub fn strlen(&self, key: &Bytes) -> usize {
        self.get(key).map(|v| v.len()).unwrap_or(0)
    }

    /// Returns all keys matching a pattern (simplified glob matching).
    ///
    /// Supported patterns:
    /// - `*` matches everything
    /// - `h*llo` matches hello, hallo, hxllo
    /// - `h?llo` matches hello, hallo, but not hllo
    /// - `h[ae]llo` matches hello and hallo, but not hillo
    ///
    /// **Warning**: This operation scans all keys and can be slow on large databases.
    pub fn keys(&self, pattern: &str) -> Vec<Bytes> {
        let mut result = Vec::new();
        let pattern = GlobPattern::new(pattern);

        for shard in &self.shards {
            let data = shard.data.read().unwrap();
            for (key, entry) in data.iter() {
                if !entry.is_expired() {
                    if let Ok(key_str) = std::str::from_utf8(key) {
                        if pattern.matches(key_str) {
                            result.push(key.clone());
                        }
                    }
                }
            }
        }

        result
    }

    /// Clears all data from the database.
    ///
    /// This is equivalent to the Redis FLUSHDB command.
    pub fn flush(&self) {
        for shard in &self.shards {
            let mut data = shard.data.write().unwrap();
            data.clear();
            let mut lists = shard.lists.write().unwrap();
            lists.clear();
        }
        self.key_count.store(0, Ordering::Relaxed);
    }

    /// Returns the approximate number of keys in the database.
    ///
    /// This is an approximation because it uses relaxed atomic ordering.
    pub fn len(&self) -> u64 {
        self.key_count.load(Ordering::Relaxed)
    }

    /// Returns true if the database is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns database statistics.
    pub fn stats(&self) -> StorageStats {
        StorageStats {
            keys: self.key_count.load(Ordering::Relaxed),
            get_ops: self.get_count.load(Ordering::Relaxed),
            set_ops: self.set_count.load(Ordering::Relaxed),
            del_ops: self.del_count.load(Ordering::Relaxed),
            expired: self.expired_count.load(Ordering::Relaxed),
        }
    }

    /// Cleans up expired keys from all shards.
    ///
    /// This is called by the background expiry sweeper.
    ///
    /// # Returns
    ///
    /// Returns the number of keys that were cleaned up.
    pub fn cleanup_expired(&self) -> u64 {
        let mut cleaned = 0u64;

        for shard in &self.shards {
            let mut data = shard.data.write().unwrap();
            let before = data.len();

            data.retain(|_, entry| !entry.is_expired());

            let removed = (before - data.len()) as u64;
            cleaned += removed;
        }

        if cleaned > 0 {
            self.key_count.fetch_sub(cleaned, Ordering::Relaxed);
            self.expired_count.fetch_add(cleaned, Ordering::Relaxed);
        }

        cleaned
    }

    // ========================================================================
    // LIST OPERATIONS
    // ========================================================================

    /// Pushes one or more values to the left (head) of a list.
    /// Creates the list if it doesn't exist.
    ///
    /// # Returns
    /// The length of the list after the push operation.
    pub fn lpush(&self, key: Bytes, values: Vec<Bytes>) -> usize {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(&key);
        let mut lists = shard.lists.write().unwrap();

        let entry = lists.entry(key).or_insert_with(ListEntry::new);

        // Check if expired, if so reset it
        if entry.is_expired() {
            *entry = ListEntry::new();
        }

        // Push values to the front (left) - each value is pushed to head in order
        // So LPUSH key a b c results in [c, b, a] (c pushed last, ends up at head)
        for value in values.into_iter() {
            entry.data.push_front(value);
        }

        entry.data.len()
    }

    /// Pushes one or more values to the right (tail) of a list.
    /// Creates the list if it doesn't exist.
    ///
    /// # Returns
    /// The length of the list after the push operation.
    pub fn rpush(&self, key: Bytes, values: Vec<Bytes>) -> usize {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(&key);
        let mut lists = shard.lists.write().unwrap();

        let entry = lists.entry(key).or_insert_with(ListEntry::new);

        // Check if expired, if so reset it
        if entry.is_expired() {
            *entry = ListEntry::new();
        }

        // Push values to the back (right)
        for value in values {
            entry.data.push_back(value);
        }

        entry.data.len()
    }

    /// Removes and returns the first element (head) of a list.
    ///
    /// # Returns
    /// The removed element, or None if the list is empty or doesn't exist.
    pub fn lpop(&self, key: &Bytes) -> Option<Bytes> {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);
        let mut lists = shard.lists.write().unwrap();

        if let Some(entry) = lists.get_mut(key) {
            if entry.is_expired() {
                lists.remove(key);
                return None;
            }
            let value = entry.data.pop_front();

            // Remove the key if the list is now empty
            if entry.data.is_empty() {
                lists.remove(key);
            }

            value
        } else {
            None
        }
    }

    /// Removes and returns the last element (tail) of a list.
    ///
    /// # Returns
    /// The removed element, or None if the list is empty or doesn't exist.
    pub fn rpop(&self, key: &Bytes) -> Option<Bytes> {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);
        let mut lists = shard.lists.write().unwrap();

        if let Some(entry) = lists.get_mut(key) {
            if entry.is_expired() {
                lists.remove(key);
                return None;
            }
            let value = entry.data.pop_back();

            // Remove the key if the list is now empty
            if entry.data.is_empty() {
                lists.remove(key);
            }

            value
        } else {
            None
        }
    }

    /// Returns the length of a list.
    ///
    /// # Returns
    /// The length of the list, or 0 if the list doesn't exist.
    pub fn llen(&self, key: &Bytes) -> usize {
        let shard = self.get_shard(key);
        let lists = shard.lists.read().unwrap();

        if let Some(entry) = lists.get(key) {
            if entry.is_expired() {
                return 0;
            }
            entry.data.len()
        } else {
            0
        }
    }

    /// Returns the element at the specified index in a list.
    /// Negative indices count from the end (-1 is the last element).
    ///
    /// # Returns
    /// The element at the index, or None if index is out of range.
    pub fn lindex(&self, key: &Bytes, index: i64) -> Option<Bytes> {
        let shard = self.get_shard(key);
        let lists = shard.lists.read().unwrap();

        if let Some(entry) = lists.get(key) {
            if entry.is_expired() {
                return None;
            }

            let len = entry.data.len() as i64;
            let actual_index = if index < 0 { len + index } else { index };

            if actual_index < 0 || actual_index >= len {
                return None;
            }

            entry.data.get(actual_index as usize).cloned()
        } else {
            None
        }
    }

    /// Returns a range of elements from a list.
    /// Both start and stop are inclusive. Negative indices count from the end.
    ///
    /// # Returns
    /// A vector of elements in the specified range.
    pub fn lrange(&self, key: &Bytes, start: i64, stop: i64) -> Vec<Bytes> {
        let shard = self.get_shard(key);
        let lists = shard.lists.read().unwrap();

        if let Some(entry) = lists.get(key) {
            if entry.is_expired() {
                return Vec::new();
            }

            let len = entry.data.len() as i64;

            // Convert negative indices
            let mut actual_start = if start < 0 { len + start } else { start };
            let mut actual_stop = if stop < 0 { len + stop } else { stop };

            // Clamp to valid range
            if actual_start < 0 {
                actual_start = 0;
            }
            if actual_stop >= len {
                actual_stop = len - 1;
            }

            if actual_start > actual_stop || actual_start >= len {
                return Vec::new();
            }

            entry
                .data
                .iter()
                .skip(actual_start as usize)
                .take((actual_stop - actual_start + 1) as usize)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Sets the element at the specified index in a list.
    /// Negative indices count from the end.
    ///
    /// # Returns
    /// Ok(()) if successful, Err with message if index is out of range or list doesn't exist.
    pub fn lset(&self, key: &Bytes, index: i64, value: Bytes) -> Result<(), String> {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);
        let mut lists = shard.lists.write().unwrap();

        if let Some(entry) = lists.get_mut(key) {
            if entry.is_expired() {
                lists.remove(key);
                return Err("ERR no such key".to_string());
            }

            let len = entry.data.len() as i64;
            let actual_index = if index < 0 { len + index } else { index };

            if actual_index < 0 || actual_index >= len {
                return Err("ERR index out of range".to_string());
            }

            entry.data[actual_index as usize] = value;
            Ok(())
        } else {
            Err("ERR no such key".to_string())
        }
    }

    /// Removes elements equal to the given value from a list.
    ///
    /// - count > 0: Remove `count` elements equal to value, from head to tail.
    /// - count < 0: Remove `|count|` elements equal to value, from tail to head.
    /// - count = 0: Remove all elements equal to value.
    ///
    /// # Returns
    /// The number of removed elements.
    pub fn lrem(&self, key: &Bytes, count: i64, value: &Bytes) -> usize {
        self.list_op_count.fetch_add(1, Ordering::Relaxed);

        let shard = self.get_shard(key);
        let mut lists = shard.lists.write().unwrap();

        if let Some(entry) = lists.get_mut(key) {
            if entry.is_expired() {
                lists.remove(key);
                return 0;
            }

            let mut removed = 0usize;
            let max_remove = if count == 0 {
                usize::MAX
            } else {
                count.unsigned_abs() as usize
            };

            if count >= 0 {
                // Remove from head to tail
                let mut i = 0;
                while i < entry.data.len() && removed < max_remove {
                    if &entry.data[i] == value {
                        entry.data.remove(i);
                        removed += 1;
                    } else {
                        i += 1;
                    }
                }
            } else {
                // Remove from tail to head
                let mut i = entry.data.len();
                while i > 0 && removed < max_remove {
                    i -= 1;
                    if &entry.data[i] == value {
                        entry.data.remove(i);
                        removed += 1;
                    }
                }
            }

            // Remove the key if the list is now empty
            if entry.data.is_empty() {
                lists.remove(key);
            }

            removed
        } else {
            0
        }
    }

    /// Checks if a key exists as a list.
    pub fn list_exists(&self, key: &Bytes) -> bool {
        let shard = self.get_shard(key);
        let lists = shard.lists.read().unwrap();

        if let Some(entry) = lists.get(key) {
            !entry.is_expired()
        } else {
            false
        }
    }

    /// Returns the type of a key ("string", "list", or "none").
    pub fn key_type(&self, key: &Bytes) -> &'static str {
        // Check string storage first
        let shard = self.get_shard(key);

        {
            let data = shard.data.read().unwrap();
            if let Some(entry) = data.get(key) {
                if !entry.is_expired() {
                    return "string";
                }
            }
        }

        {
            let lists = shard.lists.read().unwrap();
            if let Some(entry) = lists.get(key) {
                if !entry.is_expired() {
                    return "list";
                }
            }
        }

        "none"
    }

    /// Returns memory usage information (approximate).
    pub fn memory_info(&self) -> MemoryInfo {
        let mut total_keys = 0usize;
        let mut total_bytes = 0usize;

        for shard in &self.shards {
            let data = shard.data.read().unwrap();
            for (key, entry) in data.iter() {
                if !entry.is_expired() {
                    total_keys += 1;
                    // Approximate memory usage: key + value + overhead
                    total_bytes += key.len() + entry.value.len() + 64; // 64 bytes overhead estimate
                }
            }
        }

        MemoryInfo {
            keys: total_keys,
            used_memory: total_bytes,
        }
    }
}

/// Database statistics.
#[derive(Debug, Clone, Copy)]
pub struct StorageStats {
    /// Number of keys currently stored
    pub keys: u64,
    /// Total GET operations
    pub get_ops: u64,
    /// Total SET operations
    pub set_ops: u64,
    /// Total DEL operations
    pub del_ops: u64,
    /// Total expired keys cleaned up
    pub expired: u64,
}

/// Memory usage information.
#[derive(Debug, Clone, Copy)]
pub struct MemoryInfo {
    /// Number of keys
    pub keys: usize,
    /// Approximate memory used in bytes
    pub used_memory: usize,
}

/// Simple glob pattern matcher for the KEYS command.
struct GlobPattern {
    pattern: String,
}

impl GlobPattern {
    fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
        }
    }

    fn matches(&self, text: &str) -> bool {
        self.matches_recursive(self.pattern.as_bytes(), text.as_bytes())
    }

    fn matches_recursive(&self, pattern: &[u8], text: &[u8]) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }

        match pattern[0] {
            b'*' => {
                // Try matching zero or more characters
                for i in 0..=text.len() {
                    if self.matches_recursive(&pattern[1..], &text[i..]) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                // Match exactly one character
                !text.is_empty() && self.matches_recursive(&pattern[1..], &text[1..])
            }
            b'[' => {
                // Character class
                if text.is_empty() {
                    return false;
                }

                let mut i = 1;
                let mut matched = false;
                let negate = pattern.get(1) == Some(&b'^');
                if negate {
                    i += 1;
                }

                while i < pattern.len() && pattern[i] != b']' {
                    if pattern[i] == text[0] {
                        matched = true;
                    }
                    // Handle ranges like [a-z]
                    if i + 2 < pattern.len() && pattern[i + 1] == b'-' && pattern[i + 2] != b']' {
                        if text[0] >= pattern[i] && text[0] <= pattern[i + 2] {
                            matched = true;
                        }
                        i += 2;
                    }
                    i += 1;
                }

                if negate {
                    matched = !matched;
                }

                if i < pattern.len() {
                    matched && self.matches_recursive(&pattern[i + 1..], &text[1..])
                } else {
                    false
                }
            }
            b'\\' => {
                // Escape character
                if pattern.len() > 1 && !text.is_empty() && pattern[1] == text[0] {
                    self.matches_recursive(&pattern[2..], &text[1..])
                } else {
                    false
                }
            }
            c => {
                // Literal character
                !text.is_empty()
                    && c == text[0]
                    && self.matches_recursive(&pattern[1..], &text[1..])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get() {
        let engine = StorageEngine::new();

        engine.set(Bytes::from("key"), Bytes::from("value"));
        assert_eq!(engine.get(&Bytes::from("key")), Some(Bytes::from("value")));
    }

    #[test]
    fn test_get_nonexistent() {
        let engine = StorageEngine::new();
        assert_eq!(engine.get(&Bytes::from("nonexistent")), None);
    }

    #[test]
    fn test_delete() {
        let engine = StorageEngine::new();

        engine.set(Bytes::from("key"), Bytes::from("value"));
        assert!(engine.delete(&Bytes::from("key")));
        assert_eq!(engine.get(&Bytes::from("key")), None);
        assert!(!engine.delete(&Bytes::from("key"))); // Already deleted
    }

    #[test]
    fn test_exists() {
        let engine = StorageEngine::new();

        assert!(!engine.exists(&Bytes::from("key")));
        engine.set(Bytes::from("key"), Bytes::from("value"));
        assert!(engine.exists(&Bytes::from("key")));
    }

    #[test]
    fn test_expiry() {
        let engine = StorageEngine::new();

        engine.set_with_ttl(
            Bytes::from("key"),
            Bytes::from("value"),
            Duration::from_millis(50),
        );

        // Key should exist immediately
        assert!(engine.exists(&Bytes::from("key")));

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(100));

        // Key should be gone
        assert_eq!(engine.get(&Bytes::from("key")), None);
    }

    #[test]
    fn test_incr() {
        let engine = StorageEngine::new();

        // INCR on non-existent key
        assert_eq!(engine.incr(&Bytes::from("counter")), Ok(1));
        assert_eq!(engine.incr(&Bytes::from("counter")), Ok(2));

        // INCR on existing numeric string
        engine.set(Bytes::from("num"), Bytes::from("10"));
        assert_eq!(engine.incr(&Bytes::from("num")), Ok(11));

        // INCR on non-numeric string should fail
        engine.set(Bytes::from("text"), Bytes::from("hello"));
        assert!(engine.incr(&Bytes::from("text")).is_err());
    }

    #[test]
    fn test_append() {
        let engine = StorageEngine::new();

        // Append to non-existent key
        assert_eq!(engine.append(&Bytes::from("key"), &Bytes::from("Hello")), 5);

        // Append to existing key
        assert_eq!(
            engine.append(&Bytes::from("key"), &Bytes::from(" World")),
            11
        );
        assert_eq!(
            engine.get(&Bytes::from("key")),
            Some(Bytes::from("Hello World"))
        );
    }

    #[test]
    fn test_ttl() {
        let engine = StorageEngine::new();

        // No TTL on non-existent key
        assert_eq!(engine.ttl(&Bytes::from("nonexistent")), None);

        // No TTL on persistent key
        engine.set(Bytes::from("persistent"), Bytes::from("value"));
        assert_eq!(engine.ttl(&Bytes::from("persistent")), Some(-1));

        // TTL on expiring key
        engine.set_with_ttl(
            Bytes::from("expiring"),
            Bytes::from("value"),
            Duration::from_secs(100),
        );
        let ttl = engine.ttl(&Bytes::from("expiring"));
        assert!(ttl.is_some());
        assert!(ttl.unwrap() > 0 && ttl.unwrap() <= 100);
    }

    #[test]
    fn test_expire() {
        let engine = StorageEngine::new();

        engine.set(Bytes::from("key"), Bytes::from("value"));

        // Set expiry
        assert!(engine.expire(&Bytes::from("key"), Duration::from_secs(60)));

        // Check TTL
        let ttl = engine.ttl(&Bytes::from("key"));
        assert!(ttl.is_some() && ttl.unwrap() > 0);

        // Persist (remove expiry)
        assert!(engine.persist(&Bytes::from("key")));
        assert_eq!(engine.ttl(&Bytes::from("key")), Some(-1));
    }

    #[test]
    fn test_keys_pattern() {
        let engine = StorageEngine::new();

        engine.set(Bytes::from("hello"), Bytes::from("1"));
        engine.set(Bytes::from("hallo"), Bytes::from("2"));
        engine.set(Bytes::from("hxllo"), Bytes::from("3"));
        engine.set(Bytes::from("world"), Bytes::from("4"));

        // Match all
        let all = engine.keys("*");
        assert_eq!(all.len(), 4);

        // Match h*llo
        let pattern = engine.keys("h*llo");
        assert_eq!(pattern.len(), 3);

        // Match h?llo
        let pattern = engine.keys("h?llo");
        assert_eq!(pattern.len(), 3);
    }

    #[test]
    fn test_flush() {
        let engine = StorageEngine::new();

        engine.set(Bytes::from("key1"), Bytes::from("value1"));
        engine.set(Bytes::from("key2"), Bytes::from("value2"));

        assert_eq!(engine.len(), 2);

        engine.flush();

        assert_eq!(engine.len(), 0);
        assert!(engine.is_empty());
    }

    #[test]
    fn test_cleanup_expired() {
        let engine = StorageEngine::new();

        engine.set_with_ttl(
            Bytes::from("key1"),
            Bytes::from("value1"),
            Duration::from_millis(10),
        );
        engine.set_with_ttl(
            Bytes::from("key2"),
            Bytes::from("value2"),
            Duration::from_millis(10),
        );
        engine.set(Bytes::from("key3"), Bytes::from("value3")); // No expiry

        std::thread::sleep(Duration::from_millis(50));

        let cleaned = engine.cleanup_expired();
        assert_eq!(cleaned, 2);
        assert_eq!(engine.len(), 1);
        assert!(engine.exists(&Bytes::from("key3")));
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let engine = Arc::new(StorageEngine::new());
        let mut handles = vec![];

        // Spawn multiple writers
        for i in 0..10 {
            let engine = Arc::clone(&engine);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    let key = format!("key-{}-{}", i, j);
                    engine.set(Bytes::from(key.clone()), Bytes::from("value"));
                    engine.get(&Bytes::from(key));
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(engine.len(), 1000);
    }

    #[test]
    fn test_glob_pattern() {
        let pattern = GlobPattern::new("h*llo");
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("hallo"));
        assert!(pattern.matches("hllo"));
        assert!(pattern.matches("heeeello"));
        assert!(!pattern.matches("world"));

        let pattern = GlobPattern::new("h?llo");
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("hallo"));
        assert!(!pattern.matches("hllo"));
        assert!(!pattern.matches("heello"));

        let pattern = GlobPattern::new("*");
        assert!(pattern.matches(""));
        assert!(pattern.matches("anything"));

        let pattern = GlobPattern::new("h[ae]llo");
        assert!(pattern.matches("hello"));
        assert!(pattern.matches("hallo"));
        assert!(!pattern.matches("hillo"));
    }

    // ========================================================================
    // List Operation Tests
    // ========================================================================

    #[test]
    fn test_lpush_rpush() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        // LPUSH to empty list
        assert_eq!(engine.lpush(key.clone(), vec![Bytes::from("a")]), 1);
        assert_eq!(engine.lpush(key.clone(), vec![Bytes::from("b")]), 2);

        // List should be: b, a
        assert_eq!(
            engine.lrange(&key, 0, -1),
            vec![Bytes::from("b"), Bytes::from("a")]
        );

        // RPUSH
        assert_eq!(engine.rpush(key.clone(), vec![Bytes::from("c")]), 3);

        // List should be: b, a, c
        assert_eq!(
            engine.lrange(&key, 0, -1),
            vec![Bytes::from("b"), Bytes::from("a"), Bytes::from("c")]
        );

        // Multiple values at once
        assert_eq!(
            engine.lpush(key.clone(), vec![Bytes::from("x"), Bytes::from("y")]),
            5
        );
        // List should be: y, x, b, a, c (y pushed last, ends up at head)
        assert_eq!(engine.lindex(&key, 0), Some(Bytes::from("y")));
        assert_eq!(engine.lindex(&key, 1), Some(Bytes::from("x")));
    }

    #[test]
    fn test_lpop_rpop() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        // Pop from empty list
        assert_eq!(engine.lpop(&key), None);
        assert_eq!(engine.rpop(&key), None);

        // Create list: a, b, c
        engine.rpush(
            key.clone(),
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
        );

        // LPOP
        assert_eq!(engine.lpop(&key), Some(Bytes::from("a")));
        assert_eq!(engine.llen(&key), 2);

        // RPOP
        assert_eq!(engine.rpop(&key), Some(Bytes::from("c")));
        assert_eq!(engine.llen(&key), 1);

        // Pop last element
        assert_eq!(engine.lpop(&key), Some(Bytes::from("b")));
        assert_eq!(engine.llen(&key), 0);

        // List should be auto-deleted when empty
        assert!(!engine.list_exists(&key));
    }

    #[test]
    fn test_llen() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        // Empty/non-existent list
        assert_eq!(engine.llen(&key), 0);

        engine.rpush(
            key.clone(),
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
        );
        assert_eq!(engine.llen(&key), 3);
    }

    #[test]
    fn test_lindex() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        engine.rpush(
            key.clone(),
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
        );

        // Positive indices
        assert_eq!(engine.lindex(&key, 0), Some(Bytes::from("a")));
        assert_eq!(engine.lindex(&key, 1), Some(Bytes::from("b")));
        assert_eq!(engine.lindex(&key, 2), Some(Bytes::from("c")));

        // Negative indices
        assert_eq!(engine.lindex(&key, -1), Some(Bytes::from("c")));
        assert_eq!(engine.lindex(&key, -2), Some(Bytes::from("b")));
        assert_eq!(engine.lindex(&key, -3), Some(Bytes::from("a")));

        // Out of range
        assert_eq!(engine.lindex(&key, 3), None);
        assert_eq!(engine.lindex(&key, -4), None);
    }

    #[test]
    fn test_lrange() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        engine.rpush(
            key.clone(),
            vec![
                Bytes::from("a"),
                Bytes::from("b"),
                Bytes::from("c"),
                Bytes::from("d"),
                Bytes::from("e"),
            ],
        );

        // Full range
        assert_eq!(
            engine.lrange(&key, 0, -1),
            vec![
                Bytes::from("a"),
                Bytes::from("b"),
                Bytes::from("c"),
                Bytes::from("d"),
                Bytes::from("e"),
            ]
        );

        // Partial range
        assert_eq!(
            engine.lrange(&key, 1, 3),
            vec![Bytes::from("b"), Bytes::from("c"), Bytes::from("d")]
        );

        // Negative indices
        assert_eq!(
            engine.lrange(&key, -3, -1),
            vec![Bytes::from("c"), Bytes::from("d"), Bytes::from("e")]
        );

        // Out of range (should clamp)
        assert_eq!(
            engine.lrange(&key, 0, 100),
            vec![
                Bytes::from("a"),
                Bytes::from("b"),
                Bytes::from("c"),
                Bytes::from("d"),
                Bytes::from("e"),
            ]
        );

        // Invalid range (start > stop)
        assert_eq!(engine.lrange(&key, 3, 1), Vec::<Bytes>::new());
    }

    #[test]
    fn test_lset() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        engine.rpush(
            key.clone(),
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")],
        );

        // Set at valid index
        assert!(engine.lset(&key, 1, Bytes::from("B")).is_ok());
        assert_eq!(engine.lindex(&key, 1), Some(Bytes::from("B")));

        // Set with negative index
        assert!(engine.lset(&key, -1, Bytes::from("C")).is_ok());
        assert_eq!(engine.lindex(&key, -1), Some(Bytes::from("C")));

        // Out of range
        assert!(engine.lset(&key, 10, Bytes::from("X")).is_err());

        // Non-existent key
        let nonexistent = Bytes::from("nonexistent");
        assert!(engine.lset(&nonexistent, 0, Bytes::from("X")).is_err());
    }

    #[test]
    fn test_lrem() {
        let engine = StorageEngine::new();
        let key = Bytes::from("mylist");

        // Create list: a, b, a, c, a, d
        engine.rpush(
            key.clone(),
            vec![
                Bytes::from("a"),
                Bytes::from("b"),
                Bytes::from("a"),
                Bytes::from("c"),
                Bytes::from("a"),
                Bytes::from("d"),
            ],
        );

        // Remove 2 occurrences of "a" from head
        assert_eq!(engine.lrem(&key, 2, &Bytes::from("a")), 2);
        // List should be: b, c, a, d
        assert_eq!(
            engine.lrange(&key, 0, -1),
            vec![
                Bytes::from("b"),
                Bytes::from("c"),
                Bytes::from("a"),
                Bytes::from("d"),
            ]
        );

        // Remove from tail (negative count)
        engine.flush();
        engine.rpush(
            key.clone(),
            vec![
                Bytes::from("a"),
                Bytes::from("b"),
                Bytes::from("a"),
                Bytes::from("c"),
                Bytes::from("a"),
            ],
        );
        assert_eq!(engine.lrem(&key, -2, &Bytes::from("a")), 2);
        // List should be: a, b, c (removed last two "a"s)
        assert_eq!(
            engine.lrange(&key, 0, -1),
            vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")]
        );

        // Remove all (count = 0)
        engine.flush();
        engine.rpush(
            key.clone(),
            vec![Bytes::from("a"), Bytes::from("a"), Bytes::from("a")],
        );
        assert_eq!(engine.lrem(&key, 0, &Bytes::from("a")), 3);
        // List should be empty and auto-deleted
        assert!(!engine.list_exists(&key));
    }

    #[test]
    fn test_key_type() {
        let engine = StorageEngine::new();

        // Non-existent key
        assert_eq!(engine.key_type(&Bytes::from("nonexistent")), "none");

        // String key
        engine.set(Bytes::from("string_key"), Bytes::from("value"));
        assert_eq!(engine.key_type(&Bytes::from("string_key")), "string");

        // List key
        engine.rpush(Bytes::from("list_key"), vec![Bytes::from("a")]);
        assert_eq!(engine.key_type(&Bytes::from("list_key")), "list");
    }
}
