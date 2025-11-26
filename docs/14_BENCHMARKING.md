# Benchmarking FlashKV 📊

*Time: 30 minutes*

This document covers how to benchmark FlashKV, interpret results, and optimize performance. Understanding benchmarking is crucial for any systems programming project.

---

## Table of Contents

1. [Why Benchmark?](#why-benchmark)
2. [Benchmarking Tools](#benchmarking-tools)
3. [The Criterion Benchmarks](#the-criterion-benchmarks)
4. [Running Benchmarks](#running-benchmarks)
5. [Interpreting Results](#interpreting-results)
6. [Using redis-benchmark](#using-redis-benchmark)
7. [Profiling with Flamegraphs](#profiling-with-flamegraphs)
8. [Optimization Strategies](#optimization-strategies)
9. [Performance Gotchas](#performance-gotchas)

---

## Why Benchmark?

Benchmarking serves several purposes:

1. **Baseline Performance**: Know how fast your system is
2. **Regression Detection**: Catch performance regressions early
3. **Optimization Validation**: Prove that optimizations actually help
4. **Resume Material**: "Achieved X ops/sec" is concrete and impressive
5. **Comparison**: See how you compare to Redis and other databases

---

## Benchmarking Tools

### Built-in Criterion Benchmarks

FlashKV uses [Criterion.rs](https://bheisler.github.io/criterion.rs/book/) for micro-benchmarks:

```text
benches/
└── throughput.rs    # Storage engine benchmarks
```

Criterion provides:
- Statistical analysis (mean, median, std deviation)
- Automatic warmup
- Regression detection
- HTML reports

### External Tools

| Tool | Purpose |
|------|---------|
| `redis-benchmark` | Standard Redis benchmarking tool |
| `memtier_benchmark` | Advanced Redis/Memcached benchmark |
| `wrk` | HTTP benchmarking (if you add an HTTP layer) |
| `perf` | Linux profiling |
| `flamegraph` | Visual profiling |

---

## The Criterion Benchmarks

Let's examine each benchmark in `benches/throughput.rs`:

### SET Benchmarks

```rust
fn bench_set(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    let mut group = c.benchmark_group("set");
    group.throughput(Throughput::Elements(1));

    // Small values (typical)
    group.bench_function("set_small", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i));
            let value = Bytes::from("small_value");
            engine.set(key, value);
            i += 1;
        });
    });

    // Medium values (1KB)
    group.bench_function("set_medium", |b| { ... });

    // Large values (64KB)
    group.bench_function("set_large", |b| { ... });
}
```

**What we measure:**
- Operations per second for different value sizes
- How value size affects throughput

**Expected patterns:**
- Small values: ~1-5 million ops/sec
- Large values: Lower ops/sec due to memory allocation

### GET Benchmarks

```rust
fn bench_get(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    // Pre-populate with 100K entries
    for i in 0..100_000 {
        engine.set(...);
    }

    group.bench_function("get_existing", |b| {
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i % 100_000));
            black_box(engine.get(&key));  // Prevent optimization
        });
    });

    group.bench_function("get_missing", |b| {
        b.iter(|| {
            let key = Bytes::from(format!("missing:{}", i));
            black_box(engine.get(&key));  // Key doesn't exist
        });
    });
}
```

**What we measure:**
- Read performance for existing keys
- Read performance for missing keys (hash lookup only)

**Key insight:** `black_box()` prevents the compiler from optimizing away the result.

### Mixed Workload

```rust
fn bench_mixed(c: &mut Criterion) {
    group.bench_function("80_read_20_write", |b| {
        let mut i = 0u64;
        b.iter(|| {
            if i % 5 == 0 {
                // 20% writes
                engine.set(key, value);
            } else {
                // 80% reads
                engine.get(&key);
            }
            i += 1;
        });
    });
}
```

**Why this matters:** Real workloads are rarely 100% reads or writes. 80/20 is a common pattern.

### Concurrent Access

```rust
fn bench_concurrent(c: &mut Criterion) {
    group.bench_function("4_threads_mixed", |b| {
        b.iter(|| {
            let engine = Arc::new(StorageEngine::new());
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let engine = Arc::clone(&engine);
                    thread::spawn(move || {
                        for i in 0..10_000 {
                            engine.set(key, value);
                            engine.get(&key);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        });
    });
}
```

**What we measure:**
- How well sharding reduces lock contention
- Scaling behavior with multiple threads

---

## Running Benchmarks

### Basic Usage

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench -- set
cargo bench -- concurrent

# Run with verbose output
cargo bench -- --verbose
```

### Benchmark Output

```text
set/set_small           time:   [234.21 ns 235.67 ns 237.24 ns]
                        thrpt:  [4.2152 Melem/s 4.2432 Melem/s 4.2695 Melem/s]
                 change: [-1.2145% +0.1234% +1.5678%] (p = 0.12 > 0.05)
                        No change in performance detected.
```

**Understanding the output:**
- `time`: [lower bound, estimate, upper bound] per operation
- `thrpt`: Throughput (operations per second)
- `change`: Comparison to previous run
- `p`: Statistical significance (p < 0.05 means significant)

### HTML Reports

After running benchmarks, open:

```bash
open target/criterion/report/index.html
```

This shows:
- Violin plots of timing distributions
- Historical trends
- Comparison between runs

---

## Interpreting Results

### What's "Good" Performance?

| Operation | Good | Excellent | Redis |
|-----------|------|-----------|-------|
| GET (hit) | 500K/s | 1M+/s | 1-2M/s |
| SET | 300K/s | 800K+/s | 800K-1M/s |
| INCR | 400K/s | 1M+/s | 1M/s |

Note: These are single-threaded numbers. Network overhead reduces these.

### What Affects Performance?

1. **Value Size**
   - Larger values = more memory operations
   - Network bandwidth becomes bottleneck

2. **Key Distribution**
   - Hot keys cause shard contention
   - Random keys spread load evenly

3. **Operation Mix**
   - Reads are faster than writes
   - RwLock allows concurrent reads

4. **System Load**
   - Other processes affect results
   - Run benchmarks on quiet systems

### Red Flags

- **High variance**: Indicates contention or GC-like behavior
- **Bimodal distribution**: Cache effects or lock contention
- **Degradation with more threads**: Lock contention

---

## Using redis-benchmark

Since FlashKV speaks RESP, we can use Redis's own benchmark tool:

### Basic Benchmark

```bash
# Start FlashKV first
./target/release/flashkv &

# Run redis-benchmark
redis-benchmark -p 6379 -q -n 100000
```

### Specific Commands

```bash
# Benchmark only SET and GET
redis-benchmark -p 6379 -q -t set,get -n 100000

# Benchmark with specific value size (1KB)
redis-benchmark -p 6379 -q -d 1024 -n 100000

# Benchmark with pipelining
redis-benchmark -p 6379 -q -P 16 -n 100000
```

### Concurrent Clients

```bash
# 50 concurrent clients
redis-benchmark -p 6379 -q -c 50 -n 100000

# 10 clients, 16 commands per pipeline
redis-benchmark -p 6379 -q -c 10 -P 16 -n 100000
```

### Sample Output

```text
SET: 87719.30 requests per second
GET: 94339.62 requests per second
INCR: 89285.71 requests per second
```

---

## Profiling with Flamegraphs

Flamegraphs show where time is spent in your code.

### Installing cargo-flamegraph

```bash
# macOS / Linux
cargo install flamegraph

# May need additional setup for Linux
# See: https://github.com/flamegraph-rs/flamegraph
```

### Generating a Flamegraph

```bash
# Profile the benchmarks
cargo flamegraph --bench throughput -- --bench

# Or profile the server under load
cargo flamegraph --bin flashkv &
redis-benchmark -p 6379 -n 1000000 -q
```

### Reading Flamegraphs

```text
┌─────────────────────────────────────────────────────────────┐
│                       main                                  │
├───────────────────────────────┬─────────────────────────────┤
│     accept_loop (40%)         │     handle_connection (60%)  │
├───────────────┬───────────────┼──────────────┬──────────────┤
│ listener (20%)│ spawn (20%)   │ parse (25%) │ execute (35%) │
└───────────────┴───────────────┴──────────────┴──────────────┘
```

- **Width** = time spent
- **Height** = call stack depth
- **Wider bars** = optimization targets

### Common Hotspots

| Hotspot | Likely Cause | Fix |
|---------|--------------|-----|
| `parse` | RESP parsing | Optimize parser |
| `HashMap::get` | Hash computation | Use faster hasher |
| `RwLock::write` | Lock contention | Add more shards |
| `format!` | String allocation | Use pre-allocated buffers |

---

## Optimization Strategies

### 1. Reduce Allocations

**Before:**
```rust
fn get(&self, key: &str) -> Option<String> {
    let key = key.to_string();  // Allocation!
    self.data.get(&key).cloned()
}
```

**After:**
```rust
fn get(&self, key: &[u8]) -> Option<Bytes> {
    self.data.get(key).cloned()  // No allocation for lookup
}
```

### 2. Use Faster Hash Functions

```toml
# Cargo.toml
[dependencies]
ahash = "0.8"
```

```rust
use ahash::AHashMap;

// AHashMap is faster than std HashMap for most workloads
let data: AHashMap<Bytes, Entry> = AHashMap::new();
```

### 3. Tune Shard Count

```rust
// Try different values: 16, 32, 64, 128, 256
const NUM_SHARDS: usize = 64;
```

**Guidelines:**
- More shards = less contention
- Too many shards = cache inefficiency
- Sweet spot: 2-4x number of CPU cores

### 4. Buffer Pooling

```rust
// Instead of allocating new buffers
let buffer = BytesMut::with_capacity(4096);

// Use a pool
static BUFFER_POOL: Pool<BytesMut> = ...;
let buffer = BUFFER_POOL.get();
```

### 5. Batch Operations

```rust
// Instead of individual flushes
for response in responses {
    stream.write(&response)?;
    stream.flush()?;  // Expensive!
}

// Batch them
for response in responses {
    stream.write(&response)?;
}
stream.flush()?;  // One flush at the end
```

---

## Performance Gotchas

### 1. Debug vs Release Mode

```bash
# Debug mode (slow!)
cargo run

# Release mode (fast!)
cargo run --release
cargo build --release
```

Debug mode can be 10-100x slower!

### 2. Benchmark Interference

```rust
// BAD: Measuring print time
b.iter(|| {
    let result = engine.get(&key);
    println!("{:?}", result);  // Slow!
});

// GOOD: Use black_box
b.iter(|| {
    black_box(engine.get(&key));
});
```

### 3. Warm-up Effects

```rust
// First runs may be slow due to:
// - Memory allocation
// - CPU cache warming
// - JIT compilation (in some runtimes)

// Criterion handles this automatically
```

### 4. Memory Allocation Spikes

```rust
// BAD: Reallocating every operation
let mut buffer = Vec::new();
buffer.push(item);  // May reallocate

// GOOD: Pre-allocate
let mut buffer = Vec::with_capacity(1000);
buffer.push(item);  // No reallocation
```

### 5. False Sharing

```rust
// BAD: Adjacent atomics on same cache line
struct Stats {
    reads: AtomicU64,   // These might share
    writes: AtomicU64,  // a cache line!
}

// GOOD: Pad to separate cache lines
#[repr(align(64))]
struct PaddedCounter(AtomicU64);
```

---

## Benchmark Checklist

Before reporting performance numbers:

- [ ] Using release mode (`--release`)
- [ ] Running on a quiet system
- [ ] Multiple runs to confirm stability
- [ ] Comparing apples to apples (same hardware, same conditions)
- [ ] Reporting environment (CPU, RAM, OS)
- [ ] Including variance/error bars

### Sample Performance Report

```markdown
## FlashKV Performance Results

**Environment:**
- CPU: Apple M1 Pro (8 cores)
- RAM: 16GB
- OS: macOS 14.0
- Rust: 1.75.0

**Storage Engine (Criterion):**
| Operation | Throughput | Latency (p50) |
|-----------|------------|---------------|
| SET (small) | 4.2M ops/s | 238ns |
| GET (hit) | 8.1M ops/s | 123ns |
| INCR | 3.8M ops/s | 263ns |

**End-to-End (redis-benchmark):**
| Command | Throughput | Pipeline=16 |
|---------|------------|-------------|
| SET | 87K ops/s | 425K ops/s |
| GET | 94K ops/s | 512K ops/s |
```

---

## Exercises

### Exercise 1: Baseline

Run the benchmarks and record your baseline:

```bash
cargo bench 2>&1 | tee baseline.txt
```

### Exercise 2: Compare Hash Functions

1. Add `ahash` to dependencies
2. Replace `HashMap` with `AHashMap`
3. Run benchmarks and compare

### Exercise 3: Tune Shards

1. Try shard counts: 16, 32, 64, 128, 256
2. Find the optimal count for your CPU
3. Document findings

### Exercise 4: Profile Under Load

1. Generate a flamegraph
2. Identify the top 3 hotspots
3. Propose optimizations

---

## Summary

- Use Criterion for micro-benchmarks
- Use redis-benchmark for end-to-end testing
- Always use release mode
- Profile before optimizing
- Track performance over time

---

## Next Steps

Now that you understand benchmarking, try the hands-on exercises in [15_EXERCISES.md](./15_EXERCISES.md) to extend FlashKV with new features!

---

[← 13_LIB_EXPORTS.md](./13_LIB_EXPORTS.md) | [Index](./00_INDEX.md) | [15_EXERCISES.md →](./15_EXERCISES.md)