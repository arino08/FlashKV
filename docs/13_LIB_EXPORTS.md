# Library Exports (`lib.rs`) 📦

**Time: 15 minutes**

---

## Overview

The `lib.rs` file serves as the public API of the FlashKV library. It defines what modules, types, and functions are accessible to external code (or to `main.rs`). This is a crucial piece of Rust's module system.

In this document, we'll explore:
- The role of `lib.rs` in a Rust project
- How module visibility and re-exports work
- The public API design of FlashKV
- Documentation comments and their importance

---

## The Full Source

```rust
// flashkv/src/lib.rs

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
//!     let storage = Arc::new(StorageEngine::new());
//!     let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
//!     let stats = Arc::new(ConnectionStats::new());
//!     let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();
//!
//!     loop {
//!         let (stream, addr) = listener.accept().await.unwrap();
//!         let handler = CommandHandler::new(Arc::clone(&storage));
//!         let stats = Arc::clone(&stats);
//!         tokio::spawn(handle_connection(stream, addr, handler, stats));
//!     }
//! }
//! ```

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
```

---

## Part 1: Crate-Level Documentation

### Doc Comments (`//!`)

The `//!` comments at the top are **inner doc comments**. They document the item they're inside of—in this case, the entire crate.

```rust
//! # FlashKV - A High-Performance In-Memory Key-Value Database
//!
//! FlashKV is a Redis-compatible, in-memory key-value database...
```

This documentation:
- Shows up when you run `cargo doc`
- Appears on crates.io if you publish the crate
- Is rendered as Markdown

### Doc Comment Types

| Syntax | Name | Documents |
|--------|------|-----------|
| `//!` | Inner doc comment | The containing item (module, crate) |
| `///` | Outer doc comment | The following item (function, struct) |

**Example:**

```rust
//! This documents the module itself.

/// This documents the following function.
pub fn example() {}
```

---

## Part 2: Module Declarations

### The `pub mod` Statements

```rust
pub mod commands;
pub mod connection;
pub mod protocol;
pub mod storage;
```

These lines do two things:

1. **Declare** that these modules exist
2. **Export** them publicly (the `pub` keyword)

### How Rust Finds Module Files

When you write `pub mod storage;`, Rust looks for the module in one of two places:

```
src/
├── lib.rs          # Contains: pub mod storage;
├── storage.rs      # Option 1: storage.rs
└── storage/        # Option 2: storage/mod.rs
    └── mod.rs
```

FlashKV uses **Option 2** for all modules because each module has multiple files:

```
src/
├── lib.rs
├── commands/
│   ├── mod.rs
│   └── handler.rs
├── connection/
│   ├── mod.rs
│   └── handler.rs
├── protocol/
│   ├── mod.rs
│   ├── types.rs
│   └── parser.rs
└── storage/
    ├── mod.rs
    ├── engine.rs
    └── expiry.rs
```

### Visibility Cascade

```
┌─────────────────────────────────────────────────────────────┐
│                         lib.rs                              │
│                                                             │
│  pub mod storage  ─────────────► storage/mod.rs             │
│                                       │                     │
│                              pub mod engine ──► engine.rs   │
│                              pub mod expiry ──► expiry.rs   │
│                                                             │
│  External code can access:                                  │
│  - flashkv::storage::engine::StorageEngine                  │
│  - flashkv::storage::expiry::ExpirySweeper                  │
└─────────────────────────────────────────────────────────────┘
```

---

## Part 3: Re-exports

### The `pub use` Statements

```rust
pub use commands::CommandHandler;
pub use connection::{handle_connection, ConnectionStats};
pub use protocol::{ParseError, RespParser, RespValue};
pub use storage::{start_expiry_sweeper, ExpiryConfig, ExpirySweeper, StorageEngine};
```

Re-exports create **shortcuts** to commonly used types. Without them:

```rust
// Without re-exports (verbose):
use flashkv::storage::engine::StorageEngine;
use flashkv::protocol::types::RespValue;
use flashkv::connection::handler::handle_connection;

// With re-exports (convenient):
use flashkv::{StorageEngine, RespValue, handle_connection};
```

### How Re-exports Work

```
┌─────────────────────────────────────────────────────────────────────┐
│                              lib.rs                                 │
│                                                                     │
│  pub use storage::StorageEngine;                                    │
│           │                                                         │
│           │ Creates an alias:                                       │
│           │                                                         │
│           │ flashkv::StorageEngine                                  │
│           │           │                                             │
│           │           └──── Same as ────► flashkv::storage::        │
│           │                               engine::StorageEngine     │
└─────────────────────────────────────────────────────────────────────┘
```

### Benefits of Re-exports

1. **API Stability**: You can reorganize internal modules without breaking external code
2. **Convenience**: Users import from the crate root instead of deep paths
3. **Curation**: You choose what's in the "public API" surface

---

## Part 4: Constants

### Configuration Constants

```rust
/// The default port FlashKV listens on (same as Redis)
pub const DEFAULT_PORT: u16 = 6379;

/// The default host FlashKV binds to
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Version of FlashKV
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

### The `env!` Macro

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

`env!` reads an environment variable **at compile time**. `CARGO_PKG_VERSION` is automatically set by Cargo to match the version in `Cargo.toml`.

This ensures:
- Version is always accurate
- No manual synchronization needed
- Compile error if the variable doesn't exist

### Other Cargo Environment Variables

| Variable | Value |
|----------|-------|
| `CARGO_PKG_VERSION` | Package version (e.g., "0.1.0") |
| `CARGO_PKG_NAME` | Package name (e.g., "flashkv") |
| `CARGO_PKG_AUTHORS` | Package authors |
| `CARGO_PKG_DESCRIPTION` | Package description |

---

## Part 5: The Module System Deep Dive

### Understanding `lib.rs` vs `main.rs`

A Rust package can have both:

```
flashkv/
├── Cargo.toml
└── src/
    ├── lib.rs      # Library crate (flashkv)
    └── main.rs     # Binary crate (flashkv executable)
```

**Key differences:**

| `lib.rs` | `main.rs` |
|----------|-----------|
| Library entry point | Binary entry point |
| No `main()` function | Has `main()` function |
| Compiled as a library | Compiled as an executable |
| Used by `use flashkv::...` | Uses the library |

### How `main.rs` Uses `lib.rs`

```rust
// main.rs
use flashkv::commands::CommandHandler;
use flashkv::connection::{handle_connection, ConnectionStats};
use flashkv::storage::{start_expiry_sweeper, StorageEngine};
```

Even though they're in the same package, `main.rs` imports from `flashkv::` as if it were an external crate!

```
┌──────────────────┐                    ┌──────────────────┐
│     main.rs      │                    │     lib.rs       │
│                  │                    │                  │
│  Binary crate    │ ── uses ───────►   │  Library crate   │
│  (executable)    │                    │  (flashkv)       │
│                  │                    │                  │
└──────────────────┘                    └──────────────────┘
```

---

## Part 6: Visibility Modifiers

### The Visibility Spectrum

```rust
// Private (default) - only accessible within this module
fn private_function() {}

// Public - accessible from anywhere
pub fn public_function() {}

// Public within the crate only
pub(crate) fn crate_public_function() {}

// Public within parent module
pub(super) fn parent_public_function() {}

// Public within a specific path
pub(in crate::storage) fn storage_public_function() {}
```

### FlashKV's Visibility Design

```
┌─────────────────────────────────────────────────────────────────────┐
│                              Public API                             │
│  (accessible from outside the crate)                                │
│                                                                     │
│  • flashkv::StorageEngine                                           │
│  • flashkv::CommandHandler                                          │
│  • flashkv::RespParser                                              │
│  • flashkv::handle_connection                                       │
│  • flashkv::DEFAULT_PORT                                            │
│  • etc.                                                             │
├─────────────────────────────────────────────────────────────────────┤
│                           Internal API                              │
│  (accessible within the crate only)                                 │
│                                                                     │
│  • Entry struct internals                                           │
│  • Parser helper functions                                          │
│  • Shard selection logic                                            │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Part 7: Documentation Architecture

### ASCII Art in Documentation

FlashKV's lib.rs includes ASCII diagrams:

```rust
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                              FlashKV                                    │
//! │                                                                         │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
//! │  │ TCP Server  │───>│ Connection  │───>│  Command    │                 │
//! │  │ (Listener)  │    │  Handler    │    │  Handler    │                 │
//! ```
```

This renders nicely in `cargo doc` and helps users understand the architecture.

### Generating Documentation

```bash
# Generate docs
cargo doc

# Generate and open in browser
cargo doc --open

# Include private items
cargo doc --document-private-items
```

### Documentation Tests

Code in doc comments is tested by `cargo test`:

```rust
/// Adds two numbers.
///
/// # Examples
///
/// ```
/// let result = flashkv::add(2, 3);
/// assert_eq!(result, 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

The `ignore` attribute skips testing:

```rust
/// ```ignore
/// // This code won't be tested
/// ```
```

---

## Part 8: Best Practices

### 1. Keep the Public API Small

Only export what users need:

```rust
// Good: Export the main types
pub use storage::StorageEngine;

// Bad: Don't export internal implementation details
// pub use storage::engine::Shard;  // Internal!
```

### 2. Use Re-exports for Convenience

```rust
// Allow both:
use flashkv::StorageEngine;           // Convenient
use flashkv::storage::StorageEngine;  // Explicit
```

### 3. Document Everything Public

```rust
/// The core storage engine for FlashKV.
///
/// This struct provides thread-safe key-value storage with
/// support for TTL expiration.
///
/// # Example
///
/// ```
/// use flashkv::StorageEngine;
/// let engine = StorageEngine::new();
/// ```
pub struct StorageEngine { ... }
```

### 4. Organize by Feature, Not Type

```
// Good: Organized by feature
mod storage;
mod protocol;
mod commands;
mod connection;

// Bad: Organized by type
mod structs;
mod enums;
mod traits;
mod functions;
```

---

## Exercises

### Exercise 1: Add a New Re-export

Add a re-export for `ParseError` with a more descriptive name:

```rust
pub use protocol::ParseError as RespParseError;
```

Now users can write:
```rust
use flashkv::RespParseError;
```

### Exercise 2: Add a New Constant

Add a constant for the number of shards:

```rust
/// Number of shards in the storage engine
pub const NUM_SHARDS: usize = 64;
```

### Exercise 3: Create a Prelude Module

Many Rust libraries provide a "prelude" module with common imports:

```rust
// In lib.rs
pub mod prelude {
    pub use crate::StorageEngine;
    pub use crate::CommandHandler;
    pub use crate::RespValue;
    pub use crate::handle_connection;
}

// Users can now write:
use flashkv::prelude::*;
```

<details>
<summary>Solution</summary>

Add this to `lib.rs`:

```rust
/// Prelude module for convenient imports.
///
/// # Example
///
/// ```
/// use flashkv::prelude::*;
/// ```
pub mod prelude {
    pub use crate::commands::CommandHandler;
    pub use crate::connection::{handle_connection, ConnectionStats};
    pub use crate::protocol::{RespParser, RespValue};
    pub use crate::storage::{start_expiry_sweeper, StorageEngine};
    pub use crate::{DEFAULT_HOST, DEFAULT_PORT, VERSION};
}
```

</details>

---

## Summary

The `lib.rs` file is the public face of your library:

| Component | Purpose |
|-----------|---------|
| `//!` comments | Crate-level documentation |
| `pub mod` | Declare and export modules |
| `pub use` | Re-export types for convenience |
| `pub const` | Export configuration constants |

### Key Takeaways

1. **`lib.rs` defines the public API** - choose exports carefully
2. **Re-exports provide convenience** - let users import from the crate root
3. **Documentation is code** - it gets tested and rendered
4. **The `env!` macro** - reads compile-time environment variables
5. **Visibility modifiers** - control what's public at each level

---

## Next Steps

Continue to [14_BENCHMARKING.md](./14_BENCHMARKING.md) to learn about performance testing and optimization.

---

## Quick Reference

```rust
// Module declaration and export
pub mod module_name;

// Re-export a type
pub use module::Type;

// Re-export with rename
pub use module::Type as AliasName;

// Re-export multiple items
pub use module::{Type1, Type2, function};

// Compile-time environment variable
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Visibility modifiers
pub           // Public everywhere
pub(crate)    // Public within this crate
pub(super)    // Public to parent module
pub(in path)  // Public within specific path
```
