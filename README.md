<div align="center">

<pre>
   ███████████ ████                    █████      █████   ████ █████   █████
  ░░███░░░░░░█░░███                   ░░███      ░░███   ███░ ░░███   ░░███ 
   ░███   █ ░  ░███   ██████    █████  ░███████   ░███  ███    ░███    ░███ 
   ░███████    ░███  ░░░░░███  ███░░   ░███░░███  ░███████     ░███    ░███ 
   ░███░░░█    ░███   ███████ ░░█████  ░███ ░███  ░███░░███    ░░███   ███  
   ░███  ░     ░███  ███░░███  ░░░░███ ░███ ░███  ░███ ░░███    ░░░█████░   
   █████       █████░░████████ ██████  ████ █████ █████ ░░████    ░░███     
  ░░░░░       ░░░░░  ░░░░░░░░ ░░░░░░  ░░░░ ░░░░░ ░░░░░   ░░░░      ░░░      
</pre>

### A High-Performance, Redis-Compatible In-Memory Database Built from Scratch in Rust

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
[![Build](https://img.shields.io/badge/Build-Passing-brightgreen?style=for-the-badge)]()
[![Tests](https://img.shields.io/badge/Tests-69%20Passing-brightgreen?style=for-the-badge)]()

<p align="center">
  <strong>A learning-focused systems programming project demonstrating database internals, network protocols, and concurrent programming in Rust</strong>
</p>

[Features](#features) •
[Architecture](#architecture) •
[Getting Started](#getting-started) •
[Commands](#supported-commands) •
[What I Learned](#what-i-learned)

---

</div>

## About This Project

**FlashKV** is a Redis-compatible, in-memory key-value database that I built from scratch to deeply understand systems programming concepts. This isn't just another Redis clone—it's a comprehensive learning journey through database internals, network programming, protocol design, and concurrent systems.

### Why I Built This

I wanted to go beyond tutorials and actually implement the core systems that power modern databases:

- **How do databases handle thousands of concurrent connections?** → Built an async TCP server with Tokio
- **How do protocols like Redis communicate efficiently?** → Implemented a zero-copy RESP parser
- **How do you make data structures thread-safe without killing performance?** → Designed a sharded storage engine
- **How do databases handle key expiration?** → Created a dual-strategy expiry system

> 💡 **This project comes with 15 detailed documentation files** explaining every design decision, from Rust fundamentals to advanced concurrency patterns.

---

## Features

### Core Database Features
| Feature | Description |
|---------|-------------|
| **Redis Protocol Compatible** | Works with `redis-cli`, Telnet, and any Redis client library |
| **Thread-Safe Concurrent Access** | 64-shard architecture allowing parallel reads/writes |
| **TTL & Auto-Expiry** | Keys can expire automatically with lazy + active cleanup |
| **Multiple Data Types** | Strings and Lists with full Redis-compatible operations |
| **Pattern Matching** | KEYS command with glob-style pattern support (`*`, `?`, `[abc]`) |
| **Built-in Statistics** | Real-time metrics for ops/second, memory usage, and more |

### Technical Highlights
| Component | Implementation |
|-----------|----------------|
| **Async Runtime** | Tokio for handling 10,000+ concurrent connections |
| **Zero-Copy Parsing** | `bytes::Bytes` for efficient memory handling |
| **Protocol** | Full RESP (Redis Serialization Protocol) + inline commands |
| **Concurrency** | `Arc<RwLock>` with sharding to minimize contention |
| **Background Tasks** | Adaptive expiry sweeper that adjusts based on load |

---

## Architecture

```
┌────────────────────────────────────────────────────────────────────────────┐
│                                  FlashKV                                   │
│                                                                            │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐                │
│  │  TCP Server  │────▶│  Connection  │────▶│   Command    │                │
│  │  (Listener)  │     │   Handler    │     │   Handler    │                │
│  │              │     │              │     │              │                │
│  │ • Accepts    │     │ • Per-client │     │ • Parses     │                │
│  │   connections│     │   read loop  │     │   commands   │                │
│  │ • Spawns     │     │ • Buffered   │     │ • Dispatches │                │
│  │   tasks      │     │   I/O        │     │   to storage │                │
│  └──────────────┘     └──────────────┘     └──────┬───────┘                │
│                                                   │                        │
│         ┌─────────────────────────────────────────┼────────────────┐       │
│         │                                         ▼                │       │
│         │  ┌────────────────────────────────────────────────────┐  │       │
│         │  │                  StorageEngine                     │  │       │
│         │  │                                                    │  │       │
│         │  │   ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐  │  │       │
│         │  │   │ Shard 0 │ │ Shard 1 │ │ Shard 2 │ │   ...   │  │  │       │
│         │  │   │ ─────── │ │ ─────── │ │ ─────── │ │  64     │  │  │       │
│         │  │   │ RwLock  │ │ RwLock  │ │ RwLock  │ │ shards  │  │  │       │
│         │  │   │ HashMap │ │ HashMap │ │ HashMap │ │         │  │  │       │
│         │  │   │(strings)│ │(strings)│ │(strings)│ │         │  │  │       │
│         │  │   │ HashMap │ │ HashMap │ │ HashMap │ │         │  │  │       │
│         │  │   │ (lists) │ │ (lists) │ │ (lists) │ │         │  │  │       │
│         │  │   └─────────┘ └─────────┘ └─────────┘ └─────────┘  │  │       │
│         │  │                                                    │  │       │
│         │  └────────────────────────────────────────────────────┘  │       │
│         │                           ▲                              │       │
│         │                           │                              │       │
│         │  ┌────────────────────────┴───────────────────────────┐  │       │
│         │  │              ExpirySweeper (Background)            │  │       │
│         │  │  • Adaptive interval (100ms - 1s based on load)    │  │       │
│         │  │  • Scans shards for expired keys                   │  │       │
│         │  │  • Graceful shutdown support                       │  │       │
│         │  └────────────────────────────────────────────────────┘  │       │
│         └──────────────────────────────────────────────────────────┘       │
│                                                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         RESP Protocol Parser                         │  │
│  │  • Zero-copy parsing with bytes::Bytes                               │  │
│  │  • Supports: Simple Strings, Errors, Integers, Bulk Strings, Arrays  │  │
│  │  • Inline command support (plain text like "SET key value")          │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

| Decision | Why |
|----------|-----|
| **64 Shards** | Reduces lock contention—keys are distributed by hash, allowing parallel access |
| **RwLock per Shard** | Multiple readers can access data simultaneously; writers get exclusive access |
| **Separate List Storage** | Type safety—prevents accidental string operations on lists |
| **Lazy + Active Expiry** | Lazy catches expired keys on access; active reclaims memory for untouched keys |
| **VecDeque for Lists** | O(1) push/pop on both ends, perfect for LPUSH/RPUSH/LPOP/RPOP |

---

## Getting Started

### Prerequisites

- Rust 1.75+ ([Install Rust](https://rustup.rs/))

### Building

```bash
# Clone the repository
git clone https://github.com/arino08/FlashKV.git
cd flashkv

# Build in release mode (optimized)
cargo build --release

# Run tests (69 tests covering all functionality)
cargo test
```

### Running the Server

```bash
# Start with defaults (127.0.0.1:6379)
cargo run --release

# Or with custom settings
./target/release/flashkv --host 0.0.0.0 --port 6380
```

### Connecting

**Option 1: Using redis-cli**
```bash
redis-cli -p 6379
127.0.0.1:6379> PING
PONG
127.0.0.1:6379> SET greeting "Hello, World!"
OK
127.0.0.1:6379> GET greeting
"Hello, World!"
```

**Option 2: Using Telnet (inline commands)**
```bash
telnet localhost 6379
set name Ariz
+OK
get name
$4
Ariz
```

---

## Supported Commands

### String Commands (17 commands)

| Command | Syntax | Description |
|---------|--------|-------------|
| `SET` | `SET key value [EX s] [PX ms] [NX\|XX] [GET]` | Set a key with optional expiry and conditions |
| `GET` | `GET key` | Get value by key |
| `DEL` | `DEL key [key ...]` | Delete one or more keys |
| `EXISTS` | `EXISTS key [key ...]` | Check if keys exist |
| `INCR` | `INCR key` | Increment integer value by 1 |
| `INCRBY` | `INCRBY key delta` | Increment by specified amount |
| `DECR` | `DECR key` | Decrement integer value by 1 |
| `DECRBY` | `DECRBY key delta` | Decrement by specified amount |
| `APPEND` | `APPEND key value` | Append to existing string |
| `STRLEN` | `STRLEN key` | Get string length |
| `MSET` | `MSET k1 v1 [k2 v2 ...]` | Set multiple keys atomically |
| `MGET` | `MGET k1 [k2 ...]` | Get multiple values |
| `SETNX` | `SETNX key value` | Set only if key doesn't exist |
| `SETEX` | `SETEX key seconds value` | Set with expiry in seconds |
| `PSETEX` | `PSETEX key ms value` | Set with expiry in milliseconds |
| `GETSET` | `GETSET key value` | Set new value, return old |
| `GETDEL` | `GETDEL key` | Get value and delete key |

### List Commands (9 commands)

| Command | Syntax | Description |
|---------|--------|-------------|
| `LPUSH` | `LPUSH key val [val ...]` | Push to head of list |
| `RPUSH` | `RPUSH key val [val ...]` | Push to tail of list |
| `LPOP` | `LPOP key` | Remove and return from head |
| `RPOP` | `RPOP key` | Remove and return from tail |
| `LLEN` | `LLEN key` | Get list length |
| `LINDEX` | `LINDEX key index` | Get element by index (supports negative) |
| `LRANGE` | `LRANGE key start stop` | Get range of elements |
| `LSET` | `LSET key index value` | Set element at index |
| `LREM` | `LREM key count value` | Remove elements by value |

### Key Commands (10 commands)

| Command | Syntax | Description |
|---------|--------|-------------|
| `EXPIRE` | `EXPIRE key seconds` | Set TTL in seconds |
| `PEXPIRE` | `PEXPIRE key ms` | Set TTL in milliseconds |
| `EXPIREAT` | `EXPIREAT key timestamp` | Set expiry at Unix timestamp |
| `TTL` | `TTL key` | Get remaining TTL in seconds |
| `PTTL` | `PTTL key` | Get remaining TTL in milliseconds |
| `PERSIST` | `PERSIST key` | Remove expiry from key |
| `KEYS` | `KEYS pattern` | Find keys matching pattern |
| `TYPE` | `TYPE key` | Get type (string/list/none) |
| `RENAME` | `RENAME key newkey` | Rename a key |
| `RENAMENX` | `RENAMENX key newkey` | Rename only if new key doesn't exist |

### Server Commands (10 commands)

| Command | Syntax | Description |
|---------|--------|-------------|
| `PING` | `PING [message]` | Test connection |
| `ECHO` | `ECHO message` | Echo message back |
| `INFO` | `INFO [section]` | Server information |
| `DBSIZE` | `DBSIZE` | Number of keys |
| `FLUSHDB` | `FLUSHDB` | Clear entire database |
| `FLUSHALL` | `FLUSHALL` | Clear entire database |
| `COMMAND` | `COMMAND` | List available commands |
| `CONFIG` | `CONFIG GET param` | Get configuration |
| `TIME` | `TIME` | Server time |
| `DEBUG` | `DEBUG SLEEP seconds` | Debug utilities |

---

## Project Structure

```
flashkv/
├── src/
│   ├── main.rs                 # Entry point, CLI parsing, TCP server setup
│   ├── lib.rs                  # Public API exports
│   │
│   ├── protocol/               # RESP Protocol Implementation
│   │   ├── mod.rs              # Module exports
│   │   ├── types.rs            # RespValue enum, serialization
│   │   └── parser.rs           # Zero-copy parser, inline command support
│   │
│   ├── storage/                # Storage Engine
│   │   ├── mod.rs              # Module exports
│   │   ├── engine.rs           # Sharded HashMap, Entry/ListEntry, all operations
│   │   └── expiry.rs           # Background sweeper task
│   │
│   ├── commands/               # Command Handlers
│   │   ├── mod.rs              # Module exports
│   │   └── handler.rs          # 46 command implementations
│   │
│   └── connection/             # Connection Management
│       ├── mod.rs              # Module exports
│       └── handler.rs          # Per-client read loop, stats
│
├── docs/                       # Comprehensive Documentation (15 files)
│   ├── 00_INDEX.md             # Documentation index
│   ├── 01_RUST_FUNDAMENTALS.md # Rust concepts used in the project
│   ├── 02_ASYNC_PROGRAMMING.md # Tokio, async/await patterns
│   ├── 03_NETWORKING_BASICS.md # TCP, sockets, buffered I/O
│   ├── 04_RESP_PROTOCOL.md     # Redis protocol specification
│   ├── 05_PROTOCOL_TYPES.md    # RespValue implementation
│   ├── 06_PROTOCOL_PARSER.md   # Parser design and zero-copy
│   ├── 07_CONCURRENCY.md       # Arc, RwLock, sharding strategies
│   ├── 08_STORAGE_ENGINE.md    # Storage design and operations
│   ├── 09_EXPIRY_SWEEPER.md    # Background task implementation
│   ├── 10_COMMAND_HANDLER.md   # Command dispatch and handlers
│   ├── 11_CONNECTION_HANDLER.md# Client connection lifecycle
│   ├── 12_MAIN_SERVER.md       # Server bootstrap and shutdown
│   ├── 13_LIB_EXPORTS.md       # Public API design
│   ├── 14_BENCHMARKING.md      # Performance testing guide
│   └── 15_EXERCISES.md         # Extension exercises
│
├── benches/
│   └── throughput.rs           # Criterion benchmarks
│
├── Cargo.toml                  # Dependencies and metadata
└── README.md                   # You are here!
```

---

## What I Learned

Building FlashKV taught me deep, practical knowledge in several areas:

### Systems Programming
| Concept | What I Implemented |
|---------|-------------------|
| **Memory Management** | Zero-copy parsing with `bytes::Bytes`, avoiding allocations in hot paths |
| **Concurrency** | Sharded locks allowing parallel access without data races |
| **Background Tasks** | Graceful spawning and shutdown of async tasks |
| **Resource Cleanup** | Proper cleanup on connection drop and server shutdown |

### Network Programming
| Concept | What I Implemented |
|---------|-------------------|
| **TCP Servers** | Async listener accepting thousands of connections |
| **Buffered I/O** | Efficient reading with `BytesMut` buffers |
| **Protocol Parsing** | Incremental parser handling partial data |
| **Connection Lifecycle** | Per-client state management |

### Database Internals
| Concept | What I Implemented |
|---------|-------------------|
| **Storage Engines** | Thread-safe HashMap with TTL support |
| **Data Types** | Strings and Lists with type-safe operations |
| **Expiry Mechanisms** | Lazy deletion + active background sweeping |
| **Statistics Tracking** | Atomic counters for real-time metrics |

### Rust Proficiency
| Concept | Where I Used It |
|---------|-----------------|
| **Ownership & Borrowing** | Throughout—especially in the parser and storage |
| **Lifetimes** | Parser returning references into buffers |
| **Traits** | `Debug`, `Clone`, `Default` implementations |
| **Error Handling** | `Result`, `Option`, `thiserror` for custom errors |
| **Async/Await** | Tokio-based server and background tasks |
| **Smart Pointers** | `Arc`, `Box` for shared ownership |
| **Interior Mutability** | `RwLock` for concurrent access |

---

## Performance

Run benchmarks:
```bash
cargo bench
```

Or use Redis's benchmark tool:
```bash
# Start FlashKV
cargo run --release &

# Run benchmark
redis-benchmark -t set,get,incr -n 100000 -q -p 6379
```

Expected results on modern hardware:
```
SET: ~150,000 requests/second
GET: ~200,000 requests/second
INCR: ~180,000 requests/second
```

---

## Future Enhancements

The following features could be added to extend FlashKV:

- [ ] **Persistence** - RDB snapshots and AOF logging
- [ ] **Pub/Sub** - Publish/Subscribe messaging
- [ ] **Transactions** - MULTI/EXEC command blocks
- [ ] **More Data Types** - Sets, Sorted Sets, Hashes
- [ ] **Cluster Mode** - Distributed sharding across nodes
- [ ] **Lua Scripting** - Server-side scripting support

---

## Documentation

This project includes **15 detailed documentation files** covering:

1. **Rust Fundamentals** - Ownership, borrowing, lifetimes
2. **Async Programming** - Tokio, futures, task spawning
3. **Networking** - TCP, buffered I/O, connection handling
4. **Protocol Design** - RESP specification and parsing
5. **Concurrency** - Locks, atomics, sharding strategies
6. **Storage Design** - Data structures, expiry, statistics
7. **And more...**

Each file includes code examples, diagrams, and exercises.

---

## Contributing

Contributions are welcome! Whether it's:
- Bug fixes
- New features
- Documentation improvements
- Additional tests

---

## License

MIT License - feel free to use this for learning and building!

---

<div align="center">

### Built with ❤️ and 🦀 Rust

*This project was built as a learning exercise to understand database internals, network programming, and systems design.*

**[Back to Top](#flashkv)**

</div>
