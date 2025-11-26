//! Command Handler Module
//!
//! This module implements the command processing layer for FlashKV.
//! It receives parsed RESP commands, executes them against the storage engine,
//! and returns appropriate responses.
//!
//! ## Architecture
//!
//! ```text
//! Client Request
//!       │
//!       ▼
//! ┌─────────────────┐
//! │  RESP Parser    │  (protocol module)
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ CommandHandler  │  (this module)
//! │                 │
//! │  - Dispatch     │
//! │  - Validate     │
//! │  - Execute      │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ StorageEngine   │  (storage module)
//! └─────────────────┘
//! ```
//!
//! ## Supported Commands
//!
//! ### String Commands
//! - `SET`, `GET`, `DEL`, `EXISTS`
//! - `INCR`, `INCRBY`, `DECR`, `DECRBY`
//! - `APPEND`, `STRLEN`
//! - `MSET`, `MGET`
//! - `SETNX`, `SETEX`, `PSETEX`
//!
//! ### Key Commands
//! - `EXPIRE`, `PEXPIRE`, `EXPIREAT`
//! - `TTL`, `PTTL`, `PERSIST`
//! - `KEYS`, `TYPE`, `RENAME`, `RENAMENX`
//!
//! ### Server Commands
//! - `PING`, `ECHO`, `INFO`
//! - `DBSIZE`, `FLUSHDB`, `FLUSHALL`
//! - `COMMAND`, `CONFIG`, `TIME`

pub mod handler;

// Re-export the main command handler
pub use handler::CommandHandler;
