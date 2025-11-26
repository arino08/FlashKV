//! Storage Engine Module
//!
//! This module provides the core storage functionality for FlashKV.
//! It includes a thread-safe, sharded key-value store with TTL support
//! and a background expiry sweeper.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     StorageEngine                           │
//! │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │
//! │  │ Shard 0 │ │ Shard 1 │ │ Shard 2 │ │...64    │           │
//! │  │ RwLock  │ │ RwLock  │ │ RwLock  │ │ shards  │           │
//! │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           │
//! └─────────────────────────────────────────────────────────────┘
//!                            ▲
//!                            │
//!              ┌─────────────┴─────────────┐
//!              │     ExpirySweeper         │
//!              │  (Background Tokio Task)  │
//!              └───────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Sharded Storage**: 64 independent shards reduce lock contention
//! - **RwLock**: Multiple concurrent readers, exclusive writers
//! - **TTL Support**: Keys can have time-to-live expiry
//! - **Lazy Expiry**: Expired keys are cleaned on access
//! - **Active Expiry**: Background sweeper cleans orphaned expired keys
//!
//! ## Example
//!
//! ```
//! use flashkv::storage::{StorageEngine, ExpirySweeper, ExpiryConfig};
//! use bytes::Bytes;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! // Create the storage engine
//! let engine = Arc::new(StorageEngine::new());
//!
//! // Basic operations
//! engine.set(Bytes::from("name"), Bytes::from("Ariz"));
//! let value = engine.get(&Bytes::from("name"));
//! assert_eq!(value, Some(Bytes::from("Ariz")));
//!
//! // Set with TTL
//! engine.set_with_ttl(
//!     Bytes::from("session"),
//!     Bytes::from("token123"),
//!     Duration::from_secs(3600)
//! );
//! ```

pub mod engine;
pub mod expiry;

// Re-export commonly used types
pub use engine::{Entry, MemoryInfo, StorageEngine, StorageStats};
pub use expiry::{start_expiry_sweeper, ExpiryConfig, ExpirySweeper};
