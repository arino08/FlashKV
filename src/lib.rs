//! # FlashKV - A High-Performance In-Memory Key-Value Database
//!
//! FlashKV is a Redis-compatible, in-memory key-value database written in Rust.
//! It demonstrates systems programming concepts like concurrent data structures,
//! network programming, and protocol parsing.
//!
//! ## Features
//!
//! - **Redis-Compatible**: Supports the RESP protocol and common Redis commands
//! - **High Performance**: Sharded storage with RwLock for concurrent access
//! - **TTL Support**: Keys can have expiry times with automatic cleanup
//! - **Async I/O**: Built on Tokio for handling thousands of concurrent connections
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                              FlashKV                                    │
//! │                                                                         │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                  │
//! │  │ TCP Server  │───>│ Connection  │───>│  Command    │                  │
//! │  │ (Listener)  │    │  Handler    │    │  Handler    │                  │
//! │  └─────────────┘    └─────────────┘    └──────┬──────┘                  │
//! │                                               │                         │
//! │                                               ▼                         │
//! │  ┌─────────────┐    ┌──────────────────────────────────────────────┐   │
//! │  │   RESP      │    │              StorageEngine                   │   │
//! │  │   Parser    │    │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ │   │
//! │  │             │    │  │Shard 0 │ │Shard 1 │ │Shard 2 │ │...N    │ │   │
//! │  └─────────────┘    │  │RwLock  │ │RwLock  │ │RwLock  │ │shards  │ │   │
//! │                     │  └────────┘ └────────┘ └────────┘ └────────┘ │   │
//! │                     └──────────────────────────────────────────────┘   │
//! │                                               ▲                         │
//! │                                               │                         │
//! │                     ┌─────────────────────────┴───────────────────────┐ │
//! │                     │           ExpirySweeper                         │ │
//! │                     │      (Background Tokio Task)                    │ │
//! │                     └─────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```ignore
//! use flashkv::storage::{StorageEngine, start_expiry_sweeper};
//! use flashkv::commands::CommandHandler;
//! use flashkv::connection::{handle_connection, ConnectionStats};
//! use std::sync::Arc;
//! use tokio::net::TcpListener;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create the storage engine
//!     let storage = Arc::new(StorageEngine::new());
//!
//!     // Start the background expiry sweeper
//!     let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
//!
//!     // Create connection statistics
//!     let stats = Arc::new(ConnectionStats::new());
//!
//!     // Start listening for connections
//!     let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();
//!
//!     loop {
//!         let (stream, addr) = listener.accept().await.unwrap();
//!         let handler = CommandHandler::new(Arc::clone(&storage));
//!         let stats = Arc::clone(&stats);
//!
//!         tokio::spawn(handle_connection(stream, addr, handler, stats));
//!     }
//! }
//! ```
//!
//! ## Supported Commands
//!
//! ### String Commands
//! - `SET key value [EX seconds] [PX milliseconds] [NX|XX]`
//! - `GET key`
//! - `DEL key [key ...]`
//! - `EXISTS key [key ...]`
//! - `INCR key` / `INCRBY key increment`
//! - `DECR key` / `DECRBY key decrement`
//! - `APPEND key value`
//! - `STRLEN key`
//! - `MSET key value [key value ...]`
//! - `MGET key [key ...]`
//! - `SETNX key value`
//! - `SETEX key seconds value`
//!
//! ### Key Commands
//! - `EXPIRE key seconds` / `PEXPIRE key milliseconds`
//! - `TTL key` / `PTTL key`
//! - `PERSIST key`
//! - `KEYS pattern`
//! - `TYPE key`
//! - `RENAME key newkey`
//!
//! ### Server Commands
//! - `PING [message]`
//! - `ECHO message`
//! - `INFO [section]`
//! - `DBSIZE`
//! - `FLUSHDB` / `FLUSHALL`
//! - `COMMAND`
//! - `TIME`
//!
//! ## Module Overview
//!
//! - [`protocol`]: RESP protocol parser and types
//! - [`storage`]: Thread-safe storage engine with TTL support
//! - [`commands`]: Command handlers for all supported Redis commands
//! - [`connection`]: Client connection management
//!
//! ## Design Highlights
//!
//! ### Thread Safety
//!
//! The storage engine uses a sharded design with 64 independent RwLocks.
//! This allows multiple threads to read/write different keys concurrently
//! without blocking each other.
//!
//! ### Zero-Copy Parsing
//!
//! The RESP parser uses `bytes::Bytes` to avoid copying data when possible.
//! This improves performance for large values.
//!
//! ### Lazy + Active Expiry
//!
//! Keys with TTL are expired in two ways:
//! 1. **Lazy**: When a key is accessed, we check if it's expired
//! 2. **Active**: A background task periodically scans for expired keys
//!
//! This ensures memory is reclaimed even for keys that are never accessed again.

pub mod commands;
pub mod connection;
pub mod protocol;
pub mod storage;

// Re-export commonly used types for convenience
pub use commands::CommandHandler;
pub use connection::{handle_connection, ConnectionStats};
pub use protocol::{ParseError, RespParser, RespValue};
pub use storage::{start_expiry_sweeper, ExpiryConfig, ExpirySweeper, StorageEngine};

/// The default port FlashKV listens on (same as Redis)
pub const DEFAULT_PORT: u16 = 6379;

/// The default host FlashKV binds to
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Version of FlashKV
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
