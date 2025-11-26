# Storage Engine - `src/storage/engine.rs` 🧠

This document provides a deep dive into the storage engine, the heart of FlashKV that stores and retrieves all data.

---

## Table of Contents

1. [File Overview](#1-file-overview)
2. [Design Goals](#2-design-goals)
3. [The Entry Struct](#3-the-entry-struct)
4. [The Shard Struct](#4-the-shard-struct)
5. [The StorageEngine Struct](#5-the-storageengine-struct)
6. [Core Operations](#6-core-operations)
7. [TTL and Expiry](#7-ttl-and-expiry)
8. [List Operations](#8-list-operations)
9. [Advanced Operations](#9-advanced-operations)
10. [Statistics and Monitoring](#10-statistics-and-monitoring)
11. [Pattern Matching (KEYS)](#11-pattern-matching-keys)
12. [Tests](#12-tests)
13. [Exercises](#13-exercises)

---

## 1. File Overview

### Purpose

This file implements the core key-value storage with:
- Thread-safe concurrent access
- TTL (Time-To-Live) support
- Sharded architecture for performance
- Atomic statistics tracking

### Location

```
flashkv/src/storage/engine.rs
```

### Key Dependencies

```rust
use bytes::Bytes;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};
```

---

## 2. Design Goals

### Why These Choices?

| Goal | Solution |
|------|----------|
| Thread safety | RwLock per shard |
| High concurrency | 64 independent shards |
| Fast statistics | Atomic counters |
| Memory efficiency | `Bytes` for values |
| TTL support | `Instant` for expiry times |

### The Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       StorageEngine                              │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Shards (Vec<Shard>)                    │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐       ┌─────────┐   │   │
│  │  │ Shard 0 │ │ Shard 1 │ │ Shard 2 │  ...  │Shard 63 │   │   │
│  │  │ RwLock  │ │ RwLock  │ │ RwLock  │       │ RwLock  │   │   │
│  │  │ HashMap │ │ HashMap │ │ HashMap │       │ HashMap │   │   │
│  │  └─────────┘ └─────────┘ └─────────┘       └─────────┘   │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Atomic Counters (Statistics)                 │   │
│  │  key_count | get_count | set_count | del_count | expired  │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. The Entry Struct

### Definition

```rust
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
```

### What Each Field Does

| Field | Type | Purpose |
|-------|------|---------|
| `value` | `Bytes` | The actual stored data |
| `expires_at` | `Option<Instant>` | When key expires (None = never) |
| `created_at` | `Instant` | When key was first set |
| `last_accessed` | `Instant` | For future LRU eviction |

### Why `Instant` Instead of `SystemTime`?

`Instant` is monotonic - it can't go backwards:
- System clock can be adjusted (NTP, manual changes)
- `Instant` only measures elapsed time
- Perfect for TTL calculations

### Constructors

```rust
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
            expires_at: Some(now + ttl),  // Expiry = now + duration
            created_at: now,
            last_accessed: now,
        }
    }
}
```

### Expiry Check

```rust
/// Checks if this entry has expired.
#[inline]
pub fn is_expired(&self) -> bool {
    self.expires_at
        .map(|exp| Instant::now() >= exp)
        .unwrap_or(false)
}
```

**Breaking It Down**:
1. `self.expires_at` - Get the `Option<Instant>`
2. `.map(|exp| ...)` - If Some, check if current time >= expiry
3. `.unwrap_or(false)` - If None (no expiry), return false

### TTL Calculation

```rust
/// Returns the remaining TTL in milliseconds, or None if no expiry.
pub fn ttl_ms(&self) -> Option<u64> {
    self.expires_at.map(|exp| {
        let now = Instant::now();
        if now >= exp {
            0  // Already expired
        } else {
            (exp - now).as_millis() as u64
        }
    })
}
```

---

## 4. The Shard Struct

### Definition

```rust
#[derive(Debug)]
struct Shard {
    /// The actual data storage
    data: RwLock<HashMap<Bytes, Entry>>,
}

impl Shard {
    fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}
```

### Why Wrap HashMap in RwLock?

```rust
// Without lock - DATA RACE!
HashMap<Bytes, Entry>

// With lock - thread-safe
RwLock<HashMap<Bytes, Entry>>
```

The RwLock allows:
- Multiple simultaneous readers (GET operations)
- Exclusive writer access (SET, DEL operations)

---

## 5. The StorageEngine Struct

### Definition

```rust
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
}
```

### The Shard Count

```rust
const NUM_SHARDS: usize = 64;
```

Why 64?
- Powers of 2 are efficient for modulo (compiler optimizes to bitwise AND)
- 64 provides good parallelism for most workloads
- Not too many (memory overhead) or too few (contention)

### Creating a Storage Engine

```rust
impl Default for StorageEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageEngine {
    pub fn new() -> Self {
        let shards = (0..NUM_SHARDS).map(|_| Shard::new()).collect();

        Self {
            shards,
            key_count: AtomicU64::new(0),
            get_count: AtomicU64::new(0),
            set_count: AtomicU64::new(0),
            del_count: AtomicU64::new(0),
            expired_count: AtomicU64::new(0),
        }
    }
}
```

### Shard Selection

```rust
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
```

**How It Works**:
1. Hash the key bytes
2. Take modulo NUM_SHARDS (64)
3. Return reference to that shard

**Example**:
```
key = "user:123"
hash = 0xABCDEF1234567890
shard = 0xABCDEF1234567890 % 64 = 16
→ Use shards[16]
```

---

## 6. Core Operations

### SET Operation

```rust
pub fn set(&self, key: Bytes, value: Bytes) -> bool {
    // 1. Track statistics
    self.set_count.fetch_add(1, Ordering::Relaxed);

    // 2. Find the shard
    let shard = self.get_shard(&key);
    
    // 3. Acquire write lock
    let mut data = shard.data.write().unwrap();

    // 4. Check if this is a new key
    let is_new = !data.contains_key(&key);
    
    // 5. Insert the entry
    data.insert(key, Entry::new(value));

    // 6. Update key count if new
    if is_new {
        self.key_count.fetch_add(1, Ordering::Relaxed);
    }

    is_new
}
```

**Return Value**: `true` if new key, `false` if updated existing

### SET with TTL

```rust
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
```

### GET Operation (with Lazy Expiry)

```rust
pub fn get(&self, key: &Bytes) -> Option<Bytes> {
    self.get_count.fetch_add(1, Ordering::Relaxed);

    let shard = self.get_shard(key);

    // Fast path: read lock
    {
        let data = shard.data.read().unwrap();
        if let Some(entry) = data.get(key) {
            if !entry.is_expired() {
                return Some(entry.value.clone());
            }
        } else {
            return None;  // Key doesn't exist
        }
    }

    // Slow path: key exists but is expired, need write lock to remove
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
```

**The Two-Phase Approach**:
1. **Read lock (fast path)**: Check if key exists and isn't expired
2. **Write lock (slow path)**: Only if we need to remove expired key

This optimizes for the common case where keys aren't expired.

### DELETE Operation

```rust
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
```

### EXISTS Operation

```rust
pub fn exists(&self, key: &Bytes) -> bool {
    let shard = self.get_shard(key);
    let data = shard.data.read().unwrap();

    data.get(key).map(|e| !e.is_expired()).unwrap_or(false)
}
```

---

## 7. TTL and Expiry

### Setting Expiry on Existing Key

```rust
pub fn expire(&self, key: &Bytes, ttl: Duration) -> bool {
    let shard = self.get_shard(key);
    let mut data = shard.data.write().unwrap();

    if let Some(entry) = data.get_mut(key) {
        if entry.is_expired() {
            // Clean up expired key
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
```

### Removing Expiry (PERSIST)

```rust
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
            entry.expires_at = None;  // Remove expiry
            return true;
        }
    }
    false
}
```

### Getting TTL

```rust
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
            .unwrap_or(-1)  // -1 means no expiry
    })
}
```

**Return Values**:
- `None` - Key doesn't exist
- `Some(-1)` - Key exists, no expiry
- `Some(n)` - Key expires in n seconds

### Background Cleanup

```rust
pub fn cleanup_expired(&self) -> u64 {
    let mut cleaned = 0u64;

    for shard in &self.shards {
        let mut data = shard.data.write().unwrap();
        let before = data.len();

        // Remove all expired entries
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
```

This is called by the background expiry sweeper periodically.

---

## 8. List Operations

FlashKV supports Redis-compatible list data structures. Lists are stored separately from strings in their own sharded storage, using `VecDeque<Bytes>` for O(1) operations on both ends.

### ListEntry Struct

```rust
/// Represents a stored list with optional expiry time.
pub struct ListEntry {
    /// The actual list data stored as a deque for O(1) push/pop on both ends
    pub data: VecDeque<Bytes>,
    /// When this entry expires (None = never expires)
    pub expires_at: Option<Instant>,
    /// When this entry was created
    pub created_at: Instant,
}
```

### Storage Architecture

Lists are stored in a separate HashMap within each shard:

```rust
struct Shard {
    /// The actual data storage for strings
    data: RwLock<HashMap<Bytes, Entry>>,
    /// The actual data storage for lists
    lists: RwLock<HashMap<Bytes, ListEntry>>,
}
```

### LPUSH / RPUSH

Push values to the head or tail of a list:

```rust
pub fn lpush(&self, key: Bytes, values: Vec<Bytes>) -> usize {
    let shard = self.get_shard(&key);
    let mut lists = shard.lists.write().unwrap();

    let entry = lists.entry(key).or_insert_with(ListEntry::new);

    // Check if expired, if so reset it
    if entry.is_expired() {
        *entry = ListEntry::new();
    }

    // Push values to the front (left)
    for value in values.into_iter() {
        entry.data.push_front(value);
    }

    entry.data.len()
}
```

### LPOP / RPOP

Remove and return elements from either end:

```rust
pub fn lpop(&self, key: &Bytes) -> Option<Bytes> {
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
```

### LRANGE

Returns a range of elements with support for negative indices:

```rust
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
        if actual_start < 0 { actual_start = 0; }
        if actual_stop >= len { actual_stop = len - 1; }

        if actual_start > actual_stop || actual_start >= len {
            return Vec::new();
        }

        entry.data
            .iter()
            .skip(actual_start as usize)
            .take((actual_stop - actual_start + 1) as usize)
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}
```

### Key Type Detection

The storage engine can determine the type of a key:

```rust
pub fn key_type(&self, key: &Bytes) -> &'static str {
    let shard = self.get_shard(key);

    // Check string storage first
    {
        let data = shard.data.read().unwrap();
        if let Some(entry) = data.get(key) {
            if !entry.is_expired() {
                return "string";
            }
        }
    }

    // Check list storage
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
```

### Available List Operations

| Method | Description | Time Complexity |
|--------|-------------|-----------------|
| `lpush` | Push to head | O(1) per element |
| `rpush` | Push to tail | O(1) per element |
| `lpop` | Pop from head | O(1) |
| `rpop` | Pop from tail | O(1) |
| `llen` | Get length | O(1) |
| `lindex` | Get by index | O(n) |
| `lrange` | Get range | O(n) |
| `lset` | Set by index | O(n) |
| `lrem` | Remove elements | O(n) |

---

## 9. Advanced Operations

### INCR / INCRBY

```rust
pub fn incr_by(&self, key: &Bytes, delta: i64) -> Result<i64, &'static str> {
    let shard = self.get_shard(key);
    let mut data = shard.data.write().unwrap();

    // Get current value or 0
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

    // Add with overflow check
    let new_value = current
        .checked_add(delta)
        .ok_or("increment would overflow")?;

    let value_bytes = Bytes::from(new_value.to_string());

    // Preserve TTL if the key existed
    let expires_at = data.get(key)
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
```

### APPEND

```rust
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
```

### FLUSH (Clear All)

```rust
pub fn flush(&self) {
    for shard in &self.shards {
        let mut data = shard.data.write().unwrap();
        data.clear();
    }
    self.key_count.store(0, Ordering::Relaxed);
}
```

---

## 10. Statistics and Monitoring

### Getting Statistics

```rust
pub fn stats(&self) -> StorageStats {
    StorageStats {
        keys: self.key_count.load(Ordering::Relaxed),
        get_ops: self.get_count.load(Ordering::Relaxed),
        set_ops: self.set_count.load(Ordering::Relaxed),
        del_ops: self.del_count.load(Ordering::Relaxed),
        expired: self.expired_count.load(Ordering::Relaxed),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StorageStats {
    pub keys: u64,
    pub get_ops: u64,
    pub set_ops: u64,
    pub del_ops: u64,
    pub expired: u64,
}
```

### Memory Information

```rust
pub fn memory_info(&self) -> MemoryInfo {
    let mut total_keys = 0usize;
    let mut total_bytes = 0usize;

    for shard in &self.shards {
        let data = shard.data.read().unwrap();
        for (key, entry) in data.iter() {
            if !entry.is_expired() {
                total_keys += 1;
                // Approximate memory: key + value + overhead
                total_bytes += key.len() + entry.value.len() + 64;
            }
        }
    }

    MemoryInfo {
        keys: total_keys,
        used_memory: total_bytes,
    }
}
```

---

## 11. Pattern Matching (KEYS)

### The KEYS Command

```rust
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
```

### The Glob Pattern Matcher

```rust
struct GlobPattern {
    pattern: String,
}

impl GlobPattern {
    fn new(pattern: &str) -> Self {
        Self { pattern: pattern.to_string() }
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
            c => {
                // Literal character match
                !text.is_empty()
                    && c == text[0]
                    && self.matches_recursive(&pattern[1..], &text[1..])
            }
        }
    }
}
```

**Supported Patterns**:
- `*` - Match any sequence of characters
- `?` - Match exactly one character
- `[abc]` - Match one of the characters in brackets

---

## 12. Tests

### Key Test Cases

```rust
#[test]
fn test_set_and_get() {
    let engine = StorageEngine::new();
    engine.set(Bytes::from("key"), Bytes::from("value"));
    assert_eq!(engine.get(&Bytes::from("key")), Some(Bytes::from("value")));
}

#[test]
fn test_expiry() {
    let engine = StorageEngine::new();
    engine.set_with_ttl(
        Bytes::from("key"),
        Bytes::from("value"),
        Duration::from_millis(50),
    );
    assert!(engine.exists(&Bytes::from("key")));
    
    std::thread::sleep(Duration::from_millis(100));
    
    assert_eq!(engine.get(&Bytes::from("key")), None);
}

#[test]
fn test_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let engine = Arc::new(StorageEngine::new());
    let mut handles = vec![];

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
```

---

## 13. Exercises

### Exercise 1: Add GETEX Command

Implement `getex` that gets a value and sets its expiry atomically:

```rust
pub fn getex(&self, key: &Bytes, ttl: Option<Duration>) -> Option<Bytes> {
    // Get value and optionally set new TTL
}
```

### Exercise 2: Add SETRANGE Command

Implement `setrange` that overwrites part of a string:

```rust
pub fn setrange(&self, key: &Bytes, offset: usize, value: &Bytes) -> usize {
    // Overwrite bytes starting at offset
    // Return new length
}
```

### Exercise 3: Implement LRU Tracking

Update `get()` to track last access time, then implement:

```rust
pub fn evict_lru(&self, count: usize) -> usize {
    // Remove the `count` least recently accessed keys
}
```

### Exercise 4: Add Memory Limit

Add a memory limit feature:

```rust
pub fn set_with_memory_check(&self, key: Bytes, value: Bytes) -> Result<bool, &'static str> {
    // Return Err if memory limit would be exceeded
}
```

### Exercise 5: Implement SCAN

Implement cursor-based iteration (more efficient than KEYS):

```rust
pub fn scan(&self, cursor: u64, pattern: &str, count: usize) -> (u64, Vec<Bytes>) {
    // Return (next_cursor, keys)
    // cursor 0 = start, 0 returned = done
}
```

---

## Key Takeaways

1. **Sharding reduces contention** - 64 independent shards allow parallel access
2. **RwLock for read-heavy workloads** - Multiple readers, exclusive writers
3. **Lazy expiry** - Check on access, clean up expired keys
4. **Atomic counters** - Lock-free statistics tracking
5. **Two-phase GET** - Read lock first, write lock only if needed
6. **Bytes for efficiency** - Reference-counted, cheap to clone

---

## Next Steps

Now let's look at the background expiry sweeper:

**[09_EXPIRY_SWEEPER.md](./09_EXPIRY_SWEEPER.md)** - Deep dive into `src/storage/expiry.rs`!