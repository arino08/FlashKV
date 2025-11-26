# Concurrency in FlashKV 🔒

This document explains the concurrency concepts and primitives used in FlashKV to safely handle multiple clients accessing shared data simultaneously.

---

## Table of Contents

1. [The Problem: Shared Mutable State](#1-the-problem-shared-mutable-state)
2. [Rust's Concurrency Model](#2-rusts-concurrency-model)
3. [Arc: Shared Ownership](#3-arc-shared-ownership)
4. [Mutex: Mutual Exclusion](#4-mutex-mutual-exclusion)
5. [RwLock: Read-Write Lock](#5-rwlock-read-write-lock)
6. [Atomics: Lock-Free Operations](#6-atomics-lock-free-operations)
7. [Sharding: Reducing Contention](#7-sharding-reducing-contention)
8. [How FlashKV Uses These](#8-how-flashkv-uses-these)
9. [Common Pitfalls](#9-common-pitfalls)
10. [Exercises](#10-exercises)

---

## 1. The Problem: Shared Mutable State

### The Scenario

Imagine two clients connected to FlashKV simultaneously:

```
Client A: SET counter 10
Client B: INCR counter
```

What should happen?

1. Client A sets counter to 10
2. Client B increments to 11

But what if they happen at the EXACT same time?

### Race Condition

```
Without synchronization:

Thread A                    Thread B
────────                    ────────
Read counter (0)            
                            Read counter (0)
Add 10 → 10                 
                            Add 1 → 1
Write 10                    
                            Write 1
                            
Final value: 1 (WRONG! Should be 11)
```

### Data Race

In Rust terms, a data race occurs when:
1. Two or more threads access the same memory
2. At least one is a write
3. There's no synchronization

**Rust prevents data races at compile time!**

```rust
// This won't compile:
let mut data = vec![1, 2, 3];

std::thread::spawn(|| {
    data.push(4);  // ERROR: can't borrow `data` as mutable
});

data.push(5);  // Because we still have access here
```

---

## 2. Rust's Concurrency Model

### Send and Sync Traits

Rust uses marker traits to determine what can be shared:

**`Send`**: Safe to transfer to another thread
```rust
// String is Send - you can give it to another thread
let s = String::from("hello");
std::thread::spawn(move || {
    println!("{}", s);  // s moved to new thread
});
```

**`Sync`**: Safe to share references between threads
```rust
// &i32 is Sync - multiple threads can read it
let n = 42;
let r = &n;
// Can share r with other threads
```

### What's NOT Send/Sync?

```rust
use std::rc::Rc;

let rc = Rc::new(5);
std::thread::spawn(move || {
    println!("{}", rc);  // ERROR: Rc is not Send
});
```

`Rc` uses non-atomic reference counting - not safe for threads!

### The Compiler Protects You

```rust
use std::cell::RefCell;

let cell = RefCell::new(5);
std::thread::spawn(move || {
    *cell.borrow_mut() = 10;  // ERROR: RefCell is not Sync
});
```

`RefCell` uses runtime borrow checking that isn't thread-safe.

---

## 3. Arc: Shared Ownership

### The Problem

You want multiple threads to own the same data:

```rust
let data = vec![1, 2, 3];

// Thread 1 needs data
// Thread 2 needs data
// But only one can own it!
```

### The Solution: Arc (Atomic Reference Counting)

```rust
use std::sync::Arc;

let data = Arc::new(vec![1, 2, 3]);

// Clone creates a new pointer to the same data
let data1 = Arc::clone(&data);
let data2 = Arc::clone(&data);

std::thread::spawn(move || {
    println!("{:?}", data1);  // Uses reference count 1
});

std::thread::spawn(move || {
    println!("{:?}", data2);  // Uses reference count 2
});

// When all Arcs are dropped, data is freed
```

### How Arc Works

```
                    ┌──────────────────┐
                    │   Actual Data    │
                    │  vec![1, 2, 3]   │
                    │                  │
                    │  ref_count: 3    │
                    └────────▲─────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────┴────┐        ┌─────┴────┐        ┌─────┴────┐
    │  Arc 1  │        │  Arc 2   │        │  Arc 3   │
    │ (main)  │        │(thread1) │        │(thread2) │
    └─────────┘        └──────────┘        └──────────┘
```

### Arc vs Rc

| Feature | Rc | Arc |
|---------|----| ----|
| Reference counting | Non-atomic | Atomic |
| Thread-safe | No | Yes |
| Performance | Faster | Slightly slower |
| Use when | Single-threaded | Multi-threaded |

### How FlashKV Uses Arc

```rust
// In main.rs
let storage = Arc::new(StorageEngine::new());

loop {
    let (stream, addr) = listener.accept().await?;
    
    // Each connection gets its own Arc pointing to the same storage
    let storage_clone = Arc::clone(&storage);
    
    tokio::spawn(async move {
        // This task owns storage_clone
        handle_connection(stream, storage_clone).await;
    });
}
```

---

## 4. Mutex: Mutual Exclusion

### The Problem

Arc gives shared ownership, but what about modification?

```rust
let data = Arc::new(vec![1, 2, 3]);
let data_clone = Arc::clone(&data);

std::thread::spawn(move || {
    data_clone.push(4);  // ERROR: Arc<T> doesn't allow mutation!
});
```

### The Solution: Mutex

```rust
use std::sync::{Arc, Mutex};

let data = Arc::new(Mutex::new(vec![1, 2, 3]));
let data_clone = Arc::clone(&data);

std::thread::spawn(move || {
    let mut guard = data_clone.lock().unwrap();
    guard.push(4);  // OK! We have exclusive access
    // guard dropped here, lock released
});
```

### How Mutex Works

```
Thread A: lock()
          ↓
    ┌─────────────────────────────────┐
    │          Mutex                   │
    │  ┌─────────────────────────┐    │
    │  │     Protected Data      │    │
    │  │     vec![1, 2, 3]       │    │
    │  └─────────────────────────┘    │
    │                                  │
    │  State: LOCKED by Thread A       │
    └─────────────────────────────────┘
          ↑
Thread B: lock() ─── BLOCKED, waiting...

Thread A: drop(guard)
          ↓
    State: UNLOCKED
          ↓
Thread B: lock() succeeds, now LOCKED by Thread B
```

### The MutexGuard

`lock()` returns a `MutexGuard<T>`:

```rust
let mutex = Mutex::new(5);

{
    let mut guard = mutex.lock().unwrap();
    *guard = 10;
    // guard dropped here, lock released
}

// Can lock again
let guard = mutex.lock().unwrap();
```

The guard:
- Implements `Deref` and `DerefMut` for access to inner value
- Automatically unlocks when dropped (RAII)
- Prevents forgetting to unlock

### Poisoning

If a thread panics while holding a lock, the Mutex becomes "poisoned":

```rust
let mutex = Arc::new(Mutex::new(5));
let mutex_clone = Arc::clone(&mutex);

let _ = std::thread::spawn(move || {
    let _guard = mutex_clone.lock().unwrap();
    panic!("Oh no!");  // Mutex is now poisoned
}).join();

// This will return Err(PoisonError)
match mutex.lock() {
    Ok(guard) => println!("Got {}", *guard),
    Err(poisoned) => {
        // Can still access data if you want
        let guard = poisoned.into_inner();
        println!("Recovered {}", *guard);
    }
}
```

---

## 5. RwLock: Read-Write Lock

### The Problem with Mutex

Mutex is exclusive - only ONE thread at a time:

```rust
let data = Mutex::new(HashMap::new());

// Even though these are just reads, they must wait for each other!
thread1: data.lock().get("key1")
thread2: data.lock().get("key2")  // Blocks!
```

### The Solution: RwLock

RwLock allows:
- Multiple readers OR
- One writer (exclusive)

```rust
use std::sync::RwLock;

let lock = RwLock::new(5);

// Multiple readers allowed
let r1 = lock.read().unwrap();
let r2 = lock.read().unwrap();
println!("{} {}", *r1, *r2);
drop(r1);
drop(r2);

// Writer needs exclusive access
let mut w = lock.write().unwrap();
*w = 10;
```

### Read vs Write Locks

```
              ┌─────────────────────────────────────────┐
              │              RwLock                      │
              │                                          │
              │  ┌────────────────────────────────────┐ │
              │  │          Protected Data            │ │
              │  └────────────────────────────────────┘ │
              │                                          │
              │  Readers: [R1, R2, R3]   Writers: []    │
              └─────────────────────────────────────────┘
                    ↑     ↑     ↑
               Thread1 Thread2 Thread3
               (read)  (read)  (read)
               
All can read simultaneously!

──────────────────────────────────────────────────────────

              ┌─────────────────────────────────────────┐
              │              RwLock                      │
              │                                          │
              │  ┌────────────────────────────────────┐ │
              │  │          Protected Data            │ │
              │  └────────────────────────────────────┘ │
              │                                          │
              │  Readers: []   Writers: [W1]            │
              └─────────────────────────────────────────┘
                                  ↑
                              Thread4
                              (write)
               
Only one writer, no readers allowed!
```

### When to Use Which

| Use Case | Best Choice |
|----------|-------------|
| Mostly writes | Mutex |
| Mostly reads | RwLock |
| Equal reads/writes | Mutex (simpler) |
| Read-heavy workload | RwLock |

### How FlashKV Uses RwLock

```rust
// In storage/engine.rs
struct Shard {
    data: RwLock<HashMap<Bytes, Entry>>,
}

impl StorageEngine {
    pub fn get(&self, key: &Bytes) -> Option<Bytes> {
        let shard = self.get_shard(key);
        let data = shard.data.read().unwrap();  // Read lock
        // Multiple GETs can happen simultaneously!
        data.get(key).map(|e| e.value.clone())
    }
    
    pub fn set(&self, key: Bytes, value: Bytes) {
        let shard = self.get_shard(&key);
        let mut data = shard.data.write().unwrap();  // Write lock
        // Exclusive access for modification
        data.insert(key, Entry::new(value));
    }
}
```

---

## 6. Atomics: Lock-Free Operations

### The Problem

Locks have overhead:
- Context switches
- Potential for contention
- Memory barriers

For simple counters, there's a faster way!

### Atomic Types

```rust
use std::sync::atomic::{AtomicU64, Ordering};

let counter = AtomicU64::new(0);

// Atomic increment - no locks needed!
counter.fetch_add(1, Ordering::Relaxed);
counter.fetch_add(1, Ordering::Relaxed);

println!("{}", counter.load(Ordering::Relaxed));  // 2
```

### Available Atomic Types

| Type | Description |
|------|-------------|
| `AtomicBool` | Boolean |
| `AtomicI8/16/32/64` | Signed integers |
| `AtomicU8/16/32/64` | Unsigned integers |
| `AtomicUsize` | Pointer-sized unsigned |
| `AtomicPtr<T>` | Raw pointer |

### Memory Ordering

The `Ordering` parameter specifies memory synchronization:

| Ordering | Meaning | Use Case |
|----------|---------|----------|
| `Relaxed` | No synchronization | Counters where order doesn't matter |
| `Acquire` | Synchronize on load | Reading shared data |
| `Release` | Synchronize on store | Writing shared data |
| `AcqRel` | Both acquire and release | Read-modify-write |
| `SeqCst` | Total ordering | When you need strict order |

For most FlashKV uses, `Relaxed` is fine:

```rust
// Statistics counters - order doesn't matter
self.get_count.fetch_add(1, Ordering::Relaxed);
```

### How FlashKV Uses Atomics

```rust
pub struct StorageEngine {
    shards: Vec<Shard>,
    
    // Atomic counters for statistics
    key_count: AtomicU64,
    get_count: AtomicU64,
    set_count: AtomicU64,
    del_count: AtomicU64,
    expired_count: AtomicU64,
}

impl StorageEngine {
    pub fn set(&self, key: Bytes, value: Bytes) -> bool {
        self.set_count.fetch_add(1, Ordering::Relaxed);
        // ... actual set logic ...
    }
    
    pub fn stats(&self) -> StorageStats {
        StorageStats {
            keys: self.key_count.load(Ordering::Relaxed),
            get_ops: self.get_count.load(Ordering::Relaxed),
            set_ops: self.set_count.load(Ordering::Relaxed),
            // ...
        }
    }
}
```

---

## 7. Sharding: Reducing Contention

### The Problem

Even with RwLock, there's contention:

```
All operations go through ONE lock:

Thread 1: SET user:1 ──┐
Thread 2: SET user:2 ──┼──→ [Single RwLock] → HashMap
Thread 3: GET user:3 ──┤
Thread 4: GET user:4 ──┘

Writers block everyone!
```

### The Solution: Sharding

Split the data into multiple independent partitions:

```
Thread 1: SET user:1 ──→ [Lock 0] → HashMap 0
Thread 2: SET user:2 ──→ [Lock 1] → HashMap 1
Thread 3: GET user:3 ──→ [Lock 2] → HashMap 2
Thread 4: GET user:4 ──→ [Lock 3] → HashMap 3

Each shard is independent!
```

### How Sharding Works

```rust
const NUM_SHARDS: usize = 64;

struct Shard {
    data: RwLock<HashMap<Bytes, Entry>>,
}

pub struct StorageEngine {
    shards: Vec<Shard>,
}

impl StorageEngine {
    fn shard_index(&self, key: &[u8]) -> usize {
        // Hash the key to determine shard
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % NUM_SHARDS
    }
    
    fn get_shard(&self, key: &[u8]) -> &Shard {
        &self.shards[self.shard_index(key)]
    }
}
```

### Benefits

1. **Reduced contention**: Operations on different shards don't block each other
2. **Parallelism**: 64 shards = up to 64 concurrent writers
3. **Scalability**: More cores = more throughput

### Choosing Shard Count

| Factor | Consideration |
|--------|---------------|
| Too few shards | More contention |
| Too many shards | Memory overhead, iteration cost |
| CPU cores | At least match core count |
| Common choice | 16-256 shards |

FlashKV uses 64 shards - a good balance for most workloads.

---

## 8. How FlashKV Uses These

### The Complete Picture

```rust
pub struct StorageEngine {
    // 64 independent shards, each with its own RwLock
    shards: Vec<Shard>,          // Vec is accessed by index (no lock needed)
    
    // Atomic counters for statistics (no locks needed)
    key_count: AtomicU64,
    get_count: AtomicU64,
    set_count: AtomicU64,
    del_count: AtomicU64,
    expired_count: AtomicU64,
}

struct Shard {
    // RwLock protects the HashMap
    data: RwLock<HashMap<Bytes, Entry>>,
}
```

### A GET Operation

```rust
pub fn get(&self, key: &Bytes) -> Option<Bytes> {
    // 1. Increment atomic counter (no lock)
    self.get_count.fetch_add(1, Ordering::Relaxed);
    
    // 2. Find the shard (just array indexing, no lock)
    let shard = self.get_shard(key);
    
    // 3. Acquire read lock on just this shard
    let data = shard.data.read().unwrap();
    
    // 4. Look up and clone the value
    data.get(key).map(|e| e.value.clone())
    
    // 5. Lock automatically released when `data` is dropped
}
```

### A SET Operation

```rust
pub fn set(&self, key: Bytes, value: Bytes) -> bool {
    // 1. Increment atomic counter
    self.set_count.fetch_add(1, Ordering::Relaxed);
    
    // 2. Find the shard
    let shard = self.get_shard(&key);
    
    // 3. Acquire write lock on just this shard
    let mut data = shard.data.write().unwrap();
    
    // 4. Check if new and update counter
    let is_new = !data.contains_key(&key);
    if is_new {
        self.key_count.fetch_add(1, Ordering::Relaxed);
    }
    
    // 5. Insert the value
    data.insert(key, Entry::new(value));
    
    is_new
}
```

### Sharing Across Connections

```rust
// main.rs
#[tokio::main]
async fn main() {
    // Create storage wrapped in Arc (shared ownership)
    let storage = Arc::new(StorageEngine::new());
    
    // Create stats wrapped in Arc
    let stats = Arc::new(ConnectionStats::new());
    
    loop {
        let (stream, addr) = listener.accept().await?;
        
        // Clone the Arcs (just increments reference count)
        let storage = Arc::clone(&storage);
        let stats = Arc::clone(&stats);
        
        // Spawn a task with its own Arc references
        tokio::spawn(async move {
            let handler = CommandHandler::new(storage);
            handle_connection(stream, addr, handler, stats).await;
        });
    }
}
```

---

## 9. Common Pitfalls

### Pitfall 1: Deadlock

```rust
let lock_a = Mutex::new(1);
let lock_b = Mutex::new(2);

// Thread 1
let a = lock_a.lock();
let b = lock_b.lock();  // Waits for lock_b

// Thread 2
let b = lock_b.lock();
let a = lock_a.lock();  // Waits for lock_a

// DEADLOCK! Both threads waiting forever
```

**Solution**: Always acquire locks in the same order.

### Pitfall 2: Holding Locks Too Long

```rust
// BAD - holds lock during slow operation
let mut data = storage.write().unwrap();
let result = expensive_computation(&data);
data.insert(key, result);

// GOOD - minimize lock duration
let input = {
    let data = storage.read().unwrap();
    data.get(&key).clone()
};
let result = expensive_computation(&input);
{
    let mut data = storage.write().unwrap();
    data.insert(key, result);
}
```

### Pitfall 3: Lock Within Lock

```rust
// BAD - locks same mutex twice
fn process(&self) {
    let guard = self.data.lock().unwrap();
    self.helper();  // Calls lock again!
}

fn helper(&self) {
    let guard = self.data.lock().unwrap();  // DEADLOCK!
}
```

**Solution**: Use `parking_lot::ReentrantMutex` or restructure code.

### Pitfall 4: Forgetting Async Lock

```rust
// BAD - std::sync::Mutex in async code
async fn bad_async() {
    let guard = mutex.lock().unwrap();
    some_async_operation().await;  // Holding lock across await!
    // Other tasks on this thread can't proceed
}

// GOOD - use tokio::sync::Mutex for async
async fn good_async() {
    let guard = async_mutex.lock().await;
    some_async_operation().await;
    // OK - tokio::sync::Mutex is await-aware
}
```

FlashKV uses `std::sync::RwLock` which is fine because we:
1. Don't hold locks across `.await` points
2. Lock, do quick operations, release

---

## 10. Exercises

### Exercise 1: Implement a Thread-Safe Counter

Create a `Counter` struct that can be safely incremented from multiple threads:

```rust
struct Counter {
    // Your fields here
}

impl Counter {
    fn new() -> Self { ... }
    fn increment(&self) { ... }
    fn get(&self) -> u64 { ... }
}
```

Test with 10 threads each incrementing 1000 times.

### Exercise 2: Reader-Writer Statistics

Create a struct that tracks:
- Total reads
- Total writes
- Current readers
- Whether a writer is active

### Exercise 3: Simple Sharded Map

Implement a sharded HashMap with 4 shards:

```rust
struct ShardedMap<K, V> {
    // 4 shards
}

impl<K: Hash, V> ShardedMap<K, V> {
    fn get(&self, key: &K) -> Option<V> { ... }
    fn insert(&self, key: K, value: V) { ... }
}
```

### Exercise 4: Deadlock Detection

Write a program that intentionally deadlocks, then fix it.

### Exercise 5: Benchmark Sharding

Compare performance of:
1. Single Mutex<HashMap>
2. Single RwLock<HashMap>
3. 4-shard RwLock<HashMap>
4. 64-shard RwLock<HashMap>

With workload: 80% reads, 20% writes, 8 threads.

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
use std::sync::atomic::{AtomicU64, Ordering};

struct Counter {
    value: AtomicU64,
}

impl Counter {
    fn new() -> Self {
        Self { value: AtomicU64::new(0) }
    }
    
    fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }
    
    fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

fn main() {
    use std::sync::Arc;
    use std::thread;
    
    let counter = Arc::new(Counter::new());
    let mut handles = vec![];
    
    for _ in 0..10 {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                counter.increment();
            }
        }));
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    assert_eq!(counter.get(), 10_000);
    println!("Final count: {}", counter.get());
}
```

### Exercise 2 Solution

```rust
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};

struct RwStats {
    total_reads: AtomicU64,
    total_writes: AtomicU64,
    current_readers: AtomicU64,
    writer_active: AtomicBool,
}

impl RwStats {
    fn new() -> Self {
        Self {
            total_reads: AtomicU64::new(0),
            total_writes: AtomicU64::new(0),
            current_readers: AtomicU64::new(0),
            writer_active: AtomicBool::new(false),
        }
    }
    
    fn start_read(&self) {
        self.current_readers.fetch_add(1, Ordering::Acquire);
    }
    
    fn end_read(&self) {
        self.total_reads.fetch_add(1, Ordering::Relaxed);
        self.current_readers.fetch_sub(1, Ordering::Release);
    }
    
    fn start_write(&self) {
        self.writer_active.store(true, Ordering::Release);
    }
    
    fn end_write(&self) {
        self.total_writes.fetch_add(1, Ordering::Relaxed);
        self.writer_active.store(false, Ordering::Release);
    }
}
```

### Exercise 3 Solution

```rust
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::RwLock;

struct ShardedMap<K, V> {
    shards: [RwLock<HashMap<K, V>>; 4],
}

impl<K: Hash + Eq + Clone, V: Clone> ShardedMap<K, V> {
    fn new() -> Self {
        Self {
            shards: [
                RwLock::new(HashMap::new()),
                RwLock::new(HashMap::new()),
                RwLock::new(HashMap::new()),
                RwLock::new(HashMap::new()),
            ],
        }
    }
    
    fn shard_index(&self, key: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % 4
    }
    
    fn get(&self, key: &K) -> Option<V> {
        let idx = self.shard_index(key);
        let shard = self.shards[idx].read().unwrap();
        shard.get(key).cloned()
    }
    
    fn insert(&self, key: K, value: V) {
        let idx = self.shard_index(&key);
        let mut shard = self.shards[idx].write().unwrap();
        shard.insert(key, value);
    }
}
```

### Exercise 4 Solution

```rust
use std::sync::{Arc, Mutex};
use std::thread;

fn deadlock_example() {
    let lock_a = Arc::new(Mutex::new(1));
    let lock_b = Arc::new(Mutex::new(2));
    
    let a1 = Arc::clone(&lock_a);
    let b1 = Arc::clone(&lock_b);
    
    let a2 = Arc::clone(&lock_a);
    let b2 = Arc::clone(&lock_b);
    
    // This WILL deadlock!
    let t1 = thread::spawn(move || {
        let _a = a1.lock().unwrap();
        thread::sleep(std::time::Duration::from_millis(10));
        let _b = b1.lock().unwrap();
    });
    
    let t2 = thread::spawn(move || {
        let _b = b2.lock().unwrap();
        thread::sleep(std::time::Duration::from_millis(10));
        let _a = a2.lock().unwrap();
    });
    
    // These joins will hang forever!
    // t1.join().unwrap();
    // t2.join().unwrap();
}

fn fixed_example() {
    let lock_a = Arc::new(Mutex::new(1));
    let lock_b = Arc::new(Mutex::new(2));
    
    let a1 = Arc::clone(&lock_a);
    let b1 = Arc::clone(&lock_b);
    
    let a2 = Arc::clone(&lock_a);
    let b2 = Arc::clone(&lock_b);
    
    // Fixed: Both threads acquire locks in the same order (a, then b)
    let t1 = thread::spawn(move || {
        let _a = a1.lock().unwrap();
        let _b = b1.lock().unwrap();
        println!("Thread 1 got both locks");
    });
    
    let t2 = thread::spawn(move || {
        let _a = a2.lock().unwrap();  // Same order as t1
        let _b = b2.lock().unwrap();
        println!("Thread 2 got both locks");
    });
    
    t1.join().unwrap();
    t2.join().unwrap();
}
```

</details>

---

## Key Takeaways

1. **Arc** - Share ownership across threads (reference counting)
2. **Mutex** - Exclusive access (one thread at a time)
3. **RwLock** - Multiple readers OR one writer
4. **Atomics** - Lock-free operations for simple types
5. **Sharding** - Reduce contention by partitioning data
6. **RAII** - Guards automatically release locks when dropped

---

## Next Steps

Now that you understand concurrency, let's see how FlashKV's storage engine uses these concepts:

**[08_STORAGE_ENGINE.md](./08_STORAGE_ENGINE.md)** - Deep dive into `src/storage/engine.rs`!