//! Throughput Benchmark for FlashKV
//!
//! This benchmark measures the performance of the storage engine
//! under various workloads.

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use flashkv::storage::StorageEngine;
use std::sync::Arc;
use std::time::Duration;

/// Benchmark SET operations
fn bench_set(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    let mut group = c.benchmark_group("set");
    group.throughput(Throughput::Elements(1));

    group.bench_function("set_small", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i));
            let value = Bytes::from("small_value");
            engine.set(key, value);
            i += 1;
        });
    });

    group.bench_function("set_medium", |b| {
        let mut i = 0u64;
        let value = Bytes::from("x".repeat(1024)); // 1KB value
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i));
            engine.set(key, value.clone());
            i += 1;
        });
    });

    group.bench_function("set_large", |b| {
        let mut i = 0u64;
        let value = Bytes::from("x".repeat(64 * 1024)); // 64KB value
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i));
            engine.set(key, value.clone());
            i += 1;
        });
    });

    group.finish();
}

/// Benchmark GET operations
fn bench_get(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    // Pre-populate with data
    for i in 0..100_000 {
        let key = Bytes::from(format!("key:{}", i));
        let value = Bytes::from(format!("value:{}", i));
        engine.set(key, value);
    }

    let mut group = c.benchmark_group("get");
    group.throughput(Throughput::Elements(1));

    group.bench_function("get_existing", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i % 100_000));
            black_box(engine.get(&key));
            i += 1;
        });
    });

    group.bench_function("get_missing", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("missing:{}", i));
            black_box(engine.get(&key));
            i += 1;
        });
    });

    group.finish();
}

/// Benchmark mixed workload (80% reads, 20% writes)
fn bench_mixed(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    // Pre-populate
    for i in 0..10_000 {
        let key = Bytes::from(format!("key:{}", i));
        let value = Bytes::from(format!("value:{}", i));
        engine.set(key, value);
    }

    let mut group = c.benchmark_group("mixed");
    group.throughput(Throughput::Elements(1));

    group.bench_function("80_read_20_write", |b| {
        let mut i = 0u64;
        b.iter(|| {
            if i % 5 == 0 {
                // 20% writes
                let key = Bytes::from(format!("new:{}", i));
                let value = Bytes::from("value");
                engine.set(key, value);
            } else {
                // 80% reads
                let key = Bytes::from(format!("key:{}", i % 10_000));
                black_box(engine.get(&key));
            }
            i += 1;
        });
    });

    group.finish();
}

/// Benchmark INCR operations
fn bench_incr(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    let mut group = c.benchmark_group("incr");
    group.throughput(Throughput::Elements(1));

    // Single counter (high contention)
    group.bench_function("single_counter", |b| {
        let key = Bytes::from("counter");
        b.iter(|| {
            black_box(engine.incr(&key).unwrap());
        });
    });

    // Multiple counters (low contention)
    group.bench_function("multiple_counters", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("counter:{}", i % 1000));
            black_box(engine.incr(&key).unwrap());
            i += 1;
        });
    });

    group.finish();
}

/// Benchmark concurrent access
fn bench_concurrent(c: &mut Criterion) {
    use std::thread;

    let mut group = c.benchmark_group("concurrent");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("4_threads_mixed", |b| {
        b.iter(|| {
            let engine = Arc::new(StorageEngine::new());
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let engine = Arc::clone(&engine);
                    thread::spawn(move || {
                        for i in 0..10_000 {
                            let key = Bytes::from(format!("key:{}:{}", t, i));
                            let value = Bytes::from("value");
                            engine.set(key.clone(), value);
                            engine.get(&key);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }

            black_box(engine.len());
        });
    });

    group.finish();
}

/// Benchmark expiry operations
fn bench_expiry(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    let mut group = c.benchmark_group("expiry");
    group.throughput(Throughput::Elements(1));

    group.bench_function("set_with_ttl", |b| {
        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("key:{}", i));
            let value = Bytes::from("value");
            engine.set_with_ttl(key, value, Duration::from_secs(3600));
            i += 1;
        });
    });

    group.bench_function("expire_existing", |b| {
        // Pre-create keys
        for i in 0..10_000 {
            let key = Bytes::from(format!("expire:{}", i));
            engine.set(key, Bytes::from("value"));
        }

        let mut i = 0u64;
        b.iter(|| {
            let key = Bytes::from(format!("expire:{}", i % 10_000));
            engine.expire(&key, Duration::from_secs(3600));
            i += 1;
        });
    });

    group.finish();
}

/// Benchmark KEYS pattern matching
fn bench_keys(c: &mut Criterion) {
    let engine = Arc::new(StorageEngine::new());

    // Pre-populate with various key patterns
    for i in 0..1_000 {
        engine.set(Bytes::from(format!("user:{}", i)), Bytes::from("user_data"));
        engine.set(
            Bytes::from(format!("session:{}", i)),
            Bytes::from("session_data"),
        );
        engine.set(
            Bytes::from(format!("cache:{}", i)),
            Bytes::from("cache_data"),
        );
    }

    let mut group = c.benchmark_group("keys");

    group.bench_function("keys_pattern", |b| {
        b.iter(|| {
            black_box(engine.keys("user:*"));
        });
    });

    group.bench_function("keys_all", |b| {
        b.iter(|| {
            black_box(engine.keys("*"));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_set,
    bench_get,
    bench_mixed,
    bench_incr,
    bench_concurrent,
    bench_expiry,
    bench_keys,
);

criterion_main!(benches);
