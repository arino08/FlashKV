# Expiry Sweeper - `src/storage/expiry.rs` 🧹

This document explains the background expiry sweeper that automatically cleans up expired keys.

---

## Table of Contents

1. [Why Do We Need This?](#1-why-do-we-need-this)
2. [Lazy vs Active Expiry](#2-lazy-vs-active-expiry)
3. [The ExpiryConfig Struct](#3-the-expiryconfig-struct)
4. [The ExpirySweeper Struct](#4-the-expirysweeper-struct)
5. [The Sweeper Loop](#5-the-sweeper-loop)
6. [Adaptive Frequency](#6-adaptive-frequency)
7. [Graceful Shutdown](#7-graceful-shutdown)
8. [Code Walkthrough](#8-code-walkthrough)
9. [Exercises](#9-exercises)

---

## 1. Why Do We Need This?

### The Problem

With lazy expiry (checking on access), there's a problem:

```
1. Client sets key with 10 second TTL
2. Key expires after 10 seconds
3. Nobody ever accesses the key again
4. Key stays in memory FOREVER! 💀
```

This is a memory leak waiting to happen.

### The Solution

A background task periodically scans for expired keys and removes them:

```
┌─────────────────────────────────────────────────┐
│                  FlashKV                         │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │            StorageEngine                  │   │
│  │  [key1: expired] [key2: ok] [key3: exp]  │   │
│  └──────────────────────────────────────────┘   │
│                      ▲                           │
│                      │ cleanup_expired()         │
│                      │                           │
│  ┌──────────────────┴───────────────────────┐   │
│  │           ExpirySweeper                   │   │
│  │  - Runs every 100ms                       │   │
│  │  - Scans all shards                       │   │
│  │  - Removes expired keys                   │   │
│  └───────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

---

## 2. Lazy vs Active Expiry

### Lazy Expiry

Checked when a key is accessed:

```rust
pub fn get(&self, key: &Bytes) -> Option<Bytes> {
    if let Some(entry) = data.get(key) {
        if entry.is_expired() {
            data.remove(key);  // Lazy cleanup
            return None;
        }
        return Some(entry.value.clone());
    }
    None
}
```

**Pros**: Zero overhead for unused keys
**Cons**: Memory not reclaimed until access

### Active Expiry

Background task proactively cleans up:

```rust
pub fn cleanup_expired(&self) -> u64 {
    for shard in &self.shards {
        let mut data = shard.data.write().unwrap();
        data.retain(|_, entry| !entry.is_expired());
    }
    // Returns count of removed keys
}
```

**Pros**: Memory reclaimed promptly
**Cons**: Uses CPU even when idle

### FlashKV Uses Both!

1. **Lazy**: On every GET, check and remove if expired
2. **Active**: Background sweeper runs periodically

This gives us the best of both worlds.

---

## 3. The ExpiryConfig Struct

```rust
#[derive(Debug, Clone)]
pub struct ExpiryConfig {
    /// Base interval between sweeps (default: 100ms)
    pub base_interval: Duration,

    /// Minimum interval between sweeps (default: 10ms)
    pub min_interval: Duration,

    /// Maximum interval between sweeps (default: 1s)
    pub max_interval: Duration,

    /// If this fraction of scanned keys are expired, speed up sweeping
    pub speedup_threshold: f64,

    /// If this fraction of scanned keys are expired, slow down sweeping
    pub slowdown_threshold: f64,
}

impl Default for ExpiryConfig {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_millis(100),
            min_interval: Duration::from_millis(10),
            max_interval: Duration::from_secs(1),
            speedup_threshold: 0.25,  // Speed up if >25% expired
            slowdown_threshold: 0.01, // Slow down if <1% expired
        }
    }
}
```

### Configuration Options

| Field | Default | Description |
|-------|---------|-------------|
| `base_interval` | 100ms | Starting interval |
| `min_interval` | 10ms | Fastest sweep rate |
| `max_interval` | 1s | Slowest sweep rate |
| `speedup_threshold` | 0.25 | Speed up if >25% keys expired |
| `slowdown_threshold` | 0.01 | Slow down if <1% keys expired |

---

## 4. The ExpirySweeper Struct

```rust
#[derive(Debug)]
pub struct ExpirySweeper {
    /// Sender to signal shutdown
    shutdown_tx: watch::Sender<bool>,
}
```

### Why Just a Sender?

The sweeper runs as a separate Tokio task. We only need:
1. A way to signal it to stop
2. The task handles everything else

### Creating and Starting

```rust
impl ExpirySweeper {
    pub fn start(engine: Arc<StorageEngine>, config: ExpiryConfig) -> Self {
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Spawn the background task
        tokio::spawn(sweeper_loop(engine, config, shutdown_rx));

        info!("Background expiry sweeper started");

        Self { shutdown_tx }
    }
}
```

### The Watch Channel

`tokio::sync::watch` is perfect for shutdown signals:
- Multiple receivers can listen
- Receivers can check for changes efficiently
- Sender can update the value at any time

```rust
let (tx, mut rx) = watch::channel(false);

// In sweeper task:
if *rx.borrow() {
    // Shutdown requested!
    return;
}

// In main:
tx.send(true);  // Signal shutdown
```

---

## 5. The Sweeper Loop

```rust
async fn sweeper_loop(
    engine: Arc<StorageEngine>,
    config: ExpiryConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut current_interval = config.base_interval;

    loop {
        // Wait for interval OR shutdown signal
        tokio::select! {
            _ = tokio::time::sleep(current_interval) => {}
            result = shutdown_rx.changed() => {
                if result.is_err() || *shutdown_rx.borrow() {
                    debug!("Expiry sweeper received shutdown signal");
                    return;
                }
            }
        }

        // Perform cleanup
        let keys_before = engine.len();
        let expired = engine.cleanup_expired();

        // Adjust interval based on expiry rate
        // (See next section)
    }
}
```

### The Select Pattern

`tokio::select!` waits for multiple futures:

```rust
tokio::select! {
    _ = sleep(duration) => {
        // Timer fired - do cleanup
    }
    _ = shutdown_rx.changed() => {
        // Shutdown signal - exit loop
    }
}
```

Whichever completes first wins. This lets us:
1. Sleep for the interval
2. But wake up immediately if shutdown is requested

---

## 6. Adaptive Frequency

The sweeper adjusts its frequency based on how many keys are expiring:

```rust
if keys_before > 0 {
    let expiry_rate = expired as f64 / keys_before as f64;

    if expiry_rate > config.speedup_threshold {
        // Many keys expiring - speed up!
        current_interval = (current_interval / 2).max(config.min_interval);
    } else if expiry_rate < config.slowdown_threshold && expired == 0 {
        // Few keys expiring - slow down
        current_interval = (current_interval * 2).min(config.max_interval);
    }
}
```

### The Algorithm

```
If >25% of keys expired:
    interval = interval / 2 (but not below 10ms)
    
If <1% expired AND no keys expired:
    interval = interval * 2 (but not above 1s)
```

### Why Adaptive?

| Scenario | Interval | Reason |
|----------|----------|--------|
| Burst of expirations | Fast (10ms) | Clear backlog quickly |
| Normal operation | Medium (100ms) | Good balance |
| No expirations | Slow (1s) | Save CPU |

---

## 7. Graceful Shutdown

### Using Drop

```rust
impl Drop for ExpirySweeper {
    fn drop(&mut self) {
        self.stop();
    }
}

impl ExpirySweeper {
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
        info!("Background expiry sweeper stopped");
    }
}
```

### How It Works

```rust
// In main.rs
{
    let _sweeper = start_expiry_sweeper(storage);
    
    // Server runs...
    
}  // _sweeper dropped here, calls stop()

// Sweeper task receives shutdown signal and exits
```

### RAII Pattern

This is the RAII (Resource Acquisition Is Initialization) pattern:
- Resource (sweeper task) acquired in `start()`
- Resource released in `drop()`
- No manual cleanup needed!

---

## 8. Code Walkthrough

### Starting the Sweeper

```rust
// In main.rs
let storage = Arc::new(StorageEngine::new());
let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
```

### The Convenience Function

```rust
pub fn start_expiry_sweeper(engine: Arc<StorageEngine>) -> ExpirySweeper {
    ExpirySweeper::start(engine, ExpiryConfig::default())
}
```

### Custom Configuration

```rust
let config = ExpiryConfig {
    base_interval: Duration::from_millis(50),
    min_interval: Duration::from_millis(5),
    max_interval: Duration::from_millis(500),
    ..Default::default()
};

let sweeper = ExpirySweeper::start(storage, config);
```

### Logging

The sweeper logs its activity:

```rust
if expired > 0 {
    debug!(
        expired = expired,
        keys_remaining = engine.len(),
        "Expired keys cleaned up"
    );
}
```

Set `RUST_LOG=flashkv=debug` to see these messages.

---

## 9. Exercises

### Exercise 1: Add Metrics

Track sweeper statistics:

```rust
pub struct SweeperStats {
    pub sweeps_performed: AtomicU64,
    pub total_expired: AtomicU64,
    pub current_interval_ms: AtomicU64,
}
```

Add a method to retrieve these stats.

### Exercise 2: Partial Sweeps

Instead of scanning all shards, scan one shard per interval:

```rust
async fn sweeper_loop(...) {
    let mut current_shard = 0;
    
    loop {
        // Only clean one shard
        engine.cleanup_shard(current_shard);
        current_shard = (current_shard + 1) % NUM_SHARDS;
    }
}
```

### Exercise 3: Expiry Events

Add a callback when keys expire:

```rust
pub type ExpiryCallback = Box<dyn Fn(&Bytes) + Send + Sync>;

impl ExpirySweeper {
    pub fn with_callback(
        engine: Arc<StorageEngine>,
        config: ExpiryConfig,
        callback: ExpiryCallback,
    ) -> Self {
        // Call callback for each expired key
    }
}
```

### Exercise 4: Priority Expiry

Some applications want faster expiry for certain key patterns:

```rust
pub struct ExpiryConfig {
    // ...
    pub priority_patterns: Vec<String>,
    pub priority_interval: Duration,
}
```

Scan priority keys more frequently.

### Exercise 5: Lazy Expiry Only Mode

Add a configuration to disable the background sweeper:

```rust
pub enum ExpiryMode {
    LazyOnly,           // Only check on access
    ActiveOnly,         // Only background sweeper
    Hybrid,             // Both (default)
}
```

---

## Key Takeaways

1. **Dual strategy** - Lazy + active expiry for best results
2. **Adaptive frequency** - Adjust based on workload
3. **Watch channel** - Clean shutdown signaling
4. **RAII** - Drop handles cleanup automatically
5. **select!** - Wait for multiple events efficiently

---

## Next Steps

Now let's look at how commands are handled:

**[10_COMMAND_HANDLER.md](./10_COMMAND_HANDLER.md)** - Deep dive into `src/commands/handler.rs`!