# FlashKV Documentation Index 📚

Welcome to the FlashKV documentation! This guide will take you through every component of the project, explaining the concepts, code, and giving you hands-on exercises to solidify your understanding.

## Learning Path

Follow these documents in order for the best learning experience:

### 1. Foundation Concepts

| Document | Description | Time |
|----------|-------------|------|
| [01_RUST_FUNDAMENTALS.md](./01_RUST_FUNDAMENTALS.md) | Rust concepts used in this project | 45 min |
| [02_ASYNC_PROGRAMMING.md](./02_ASYNC_PROGRAMMING.md) | Understanding async/await and Tokio | 30 min |
| [03_NETWORKING_BASICS.md](./03_NETWORKING_BASICS.md) | TCP, sockets, and network programming | 30 min |

### 2. Protocol Layer

| Document | Description | Time |
|----------|-------------|------|
| [04_RESP_PROTOCOL.md](./04_RESP_PROTOCOL.md) | Understanding the Redis protocol | 20 min |
| [05_PROTOCOL_TYPES.md](./05_PROTOCOL_TYPES.md) | `src/protocol/types.rs` explained | 40 min |
| [06_PROTOCOL_PARSER.md](./06_PROTOCOL_PARSER.md) | `src/protocol/parser.rs` explained | 60 min |

### 3. Storage Layer

| Document | Description | Time |
|----------|-------------|------|
| [07_CONCURRENCY.md](./07_CONCURRENCY.md) | Thread safety, locks, and atomics | 45 min |
| [08_STORAGE_ENGINE.md](./08_STORAGE_ENGINE.md) | `src/storage/engine.rs` explained | 90 min |
| [09_EXPIRY_SWEEPER.md](./09_EXPIRY_SWEEPER.md) | `src/storage/expiry.rs` explained | 30 min |

### 4. Command Layer

| Document | Description | Time |
|----------|-------------|------|
| [10_COMMAND_HANDLER.md](./10_COMMAND_HANDLER.md) | `src/commands/handler.rs` explained | 60 min |

### 5. Connection Layer

| Document | Description | Time |
|----------|-------------|------|
| [11_CONNECTION_HANDLER.md](./11_CONNECTION_HANDLER.md) | `src/connection/handler.rs` explained | 45 min |

### 6. Server

| Document | Description | Time |
|----------|-------------|------|
| [12_MAIN_SERVER.md](./12_MAIN_SERVER.md) | `src/main.rs` explained | 30 min |
| [13_LIB_EXPORTS.md](./13_LIB_EXPORTS.md) | `src/lib.rs` explained | 15 min |

### 7. Advanced Topics

| Document | Description | Time |
|----------|-------------|------|
| [14_BENCHMARKING.md](./14_BENCHMARKING.md) | Performance testing and optimization | 30 min |
| [15_EXERCISES.md](./15_EXERCISES.md) | Hands-on projects to extend FlashKV | 2+ hours |

---

## Project Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              FlashKV                                    │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                         main.rs                                  │   │
│  │                    (TCP Server Loop)                             │   │
│  └──────────────────────────┬──────────────────────────────────────┘   │
│                             │                                           │
│                             │ For each connection...                    │
│                             ▼                                           │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    connection/handler.rs                         │   │
│  │              (Per-Client Read/Write Loop)                        │   │
│  └──────────────────────────┬──────────────────────────────────────┘   │
│                             │                                           │
│              ┌──────────────┴──────────────┐                           │
│              ▼                              ▼                           │
│  ┌─────────────────────┐       ┌─────────────────────┐                 │
│  │  protocol/parser.rs │       │ commands/handler.rs │                 │
│  │   (RESP Parser)     │       │ (Command Executor)  │                 │
│  └─────────────────────┘       └──────────┬──────────┘                 │
│              │                             │                            │
│              ▼                             ▼                            │
│  ┌─────────────────────┐       ┌─────────────────────────────────┐     │
│  │  protocol/types.rs  │       │       storage/engine.rs         │     │
│  │   (RESP Values)     │       │    (Sharded Thread-Safe DB)     │     │
│  └─────────────────────┘       └──────────────────────────────────┘    │
│                                             ▲                           │
│                                             │                           │
│                                ┌────────────┴────────────┐              │
│                                │   storage/expiry.rs     │              │
│                                │ (Background Sweeper)    │              │
│                                └─────────────────────────┘              │
└─────────────────────────────────────────────────────────────────────────┘
```

## Data Flow

1. **Client connects** → TCP socket accepted in `main.rs`
2. **Connection spawned** → New Tokio task created with `connection/handler.rs`
3. **Bytes received** → Raw TCP bytes read into buffer
4. **Parsing** → `protocol/parser.rs` converts bytes to `RespValue`
5. **Command execution** → `commands/handler.rs` processes the command
6. **Storage operation** → `storage/engine.rs` reads/writes data
7. **Response** → `RespValue` serialized back to bytes
8. **Bytes sent** → Response written to TCP socket

## Key Concepts You'll Learn

### Systems Programming
- Memory management without garbage collection
- Zero-copy data handling
- Buffer management

### Concurrency
- Thread safety with `Arc<RwLock<T>>`
- Atomic operations
- Sharding for reduced contention

### Networking
- TCP socket programming
- Protocol design and parsing
- Handling partial reads

### Async Programming
- Tokio runtime
- async/await syntax
- Task spawning and management

### Database Internals
- Key-value storage
- TTL and expiration
- Background maintenance tasks

---

## Quick Reference

### Running FlashKV

```bash
# Build
cargo build --release

# Run
./target/release/flashkv

# Connect with redis-cli
redis-cli -p 6379
```

### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test storage::engine

# With output
cargo test -- --nocapture
```

### Running Benchmarks

```bash
cargo bench
```

---

## Prerequisites

Before diving in, you should be comfortable with:

- Basic Rust syntax (variables, functions, structs, enums)
- Pattern matching
- Error handling with `Result` and `Option`
- Basic understanding of references and borrowing

If you need a refresher, check out [The Rust Book](https://doc.rust-lang.org/book/).

---

**Ready to begin? Start with [01_RUST_FUNDAMENTALS.md](./01_RUST_FUNDAMENTALS.md)!**