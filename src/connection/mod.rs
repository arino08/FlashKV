//! Connection Handler Module
//!
//! This module manages individual client connections to FlashKV.
//! Each client connection is handled by its own async task, allowing
//! the server to handle thousands of concurrent clients efficiently.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     TCP Listener                            │
//! │                    (main.rs)                                │
//! └──────────────────────┬──────────────────────────────────────┘
//!                        │
//!                        │ accept()
//!                        ▼
//!           ┌────────────────────────┐
//!           │   For each client...   │
//!           └────────────┬───────────┘
//!                        │
//!                        │ spawn task
//!                        ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 ConnectionHandler                           │
//! │                                                             │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
//! │  │ Read bytes  │───>│ Parse RESP  │───>│ Execute cmd │     │
//! │  └─────────────┘    └─────────────┘    └─────────────┘     │
//! │                                               │             │
//! │                                               ▼             │
//! │                                      ┌─────────────┐        │
//! │                                      │ Send resp   │        │
//! │                                      └─────────────┘        │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Async I/O**: Uses Tokio for non-blocking network operations
//! - **Buffer Management**: Efficient BytesMut buffer for incoming data
//! - **Pipelining**: Supports multiple commands in a single TCP packet
//! - **Statistics**: Tracks connection and command metrics
//!
//! ## Example
//!
//! ```ignore
//! use flashkv::connection::{handle_connection, ConnectionStats};
//! use flashkv::commands::CommandHandler;
//! use flashkv::storage::StorageEngine;
//! use std::sync::Arc;
//!
//! let storage = Arc::new(StorageEngine::new());
//! let stats = Arc::new(ConnectionStats::new());
//! let handler = CommandHandler::new(storage);
//!
//! // For each accepted connection...
//! let (stream, addr) = listener.accept().await?;
//! tokio::spawn(handle_connection(stream, addr, handler, stats));
//! ```

pub mod handler;

// Re-export commonly used types
pub use handler::{handle_connection, ConnectionError, ConnectionHandler, ConnectionStats};
