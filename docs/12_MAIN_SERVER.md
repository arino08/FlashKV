# 12. Main Server Entry Point 🚀

**File:** `src/main.rs`  
**Time:** 30 minutes  
**Difficulty:** ⭐⭐ (Intermediate)

---

## Overview

The `main.rs` file is the entry point for FlashKV. It orchestrates all the components we've built:

1. Parses command-line arguments
2. Sets up logging infrastructure
3. Creates the storage engine
4. Starts the background expiry sweeper
5. Binds the TCP listener
6. Runs the main accept loop
7. Handles graceful shutdown

This is where everything comes together!

---

## Table of Contents

1. [Module Structure](#module-structure)
2. [Configuration](#configuration)
3. [The Main Function](#the-main-function)
4. [Accept Loop](#accept-loop)
5. [Graceful Shutdown](#graceful-shutdown)
6. [Data Flow](#data-flow)
7. [Exercises](#exercises)

---

## Module Structure

```
main.rs
├── Config struct           # Server configuration
│   ├── host: String
│   └── port: u16
├── print_help()            # Help message
├── print_banner()          # Startup banner
├── main()                  # Entry point
└── accept_loop()           # Connection accept loop
```

---

## Configuration

### The Config Struct

```rust
struct Config {
    /// Host to bind to
    host: String,
    /// Port to listen on
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6379,
        }
    }
}
```

**Design Notes:**
- Default values match Redis (127.0.0.1:6379)
- `127.0.0.1` means localhost only (more secure)
- Use `0.0.0.0` to listen on all interfaces

### Command-Line Argument Parsing

```rust
impl Config {
    fn from_args() -> Self {
        let mut config = Config::default();
        let args: Vec<String> = std::env::args().collect();

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--host" | "-h" => {
                    if i + 1 < args.len() {
                        config.host = args[i + 1].clone();
                        i += 2;
                    } else {
                        eprintln!("Error: --host requires a value");
                        std::process::exit(1);
                    }
                }
                "--port" | "-p" => {
                    if i + 1 < args.len() {
                        config.port = args[i + 1].parse().unwrap_or_else(|_| {
                            eprintln!("Error: invalid port number");
                            std::process::exit(1);
                        });
                        i += 2;
                    } else {
                        eprintln!("Error: --port requires a value");
                        std::process::exit(1);
                    }
                }
                "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--version" | "-v" => {
                    println!("FlashKV version {}", flashkv::VERSION);
                    std::process::exit(0);
                }
                _ => {
                    eprintln!("Unknown argument: {}", args[i]);
                    print_help();
                    std::process::exit(1);
                }
            }
        }

        config
    }
}
```

**Why Manual Parsing?**

We parse arguments manually here to keep dependencies minimal. In a production application, you might use:

- **clap** - Full-featured argument parser
- **structopt** - Derive-based argument parsing
- **argh** - Simple derive-based parser from Google

**Usage Examples:**

```bash
# Default configuration
./flashkv

# Custom port
./flashkv --port 6380
./flashkv -p 6380

# Custom host (listen on all interfaces)
./flashkv --host 0.0.0.0

# Both
./flashkv --host 0.0.0.0 --port 6380

# Help
./flashkv --help

# Version
./flashkv --version
```

---

## The Main Function

### The #[tokio::main] Macro

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ...
}
```

This macro transforms our async main function into a synchronous one that sets up the Tokio runtime. It's equivalent to:

```rust
fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        // Your async code here
    })
}
```

### Initialization Steps

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Parse command-line arguments
    let config = Config::from_args();

    // 2. Set up logging
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    // 3. Print the banner
    print_banner(&config);

    // 4. Create the storage engine (shared across all connections)
    let storage = Arc::new(StorageEngine::new());
    info!("Storage engine initialized with 64 shards");

    // 5. Start the background expiry sweeper
    let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
    info!("Background expiry sweeper started");

    // 6. Create connection statistics
    let stats = Arc::new(ConnectionStats::new());

    // 7. Bind the TCP listener
    let listener = TcpListener::bind(config.bind_address()).await?;
    info!("Listening on {}", config.bind_address());

    // 8. Set up graceful shutdown
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("Shutdown signal received, stopping server...");
    };

    // 9. Main accept loop
    tokio::select! {
        _ = accept_loop(listener, storage, stats) => {}
        _ = shutdown => {}
    }

    info!("Server shutdown complete");
    Ok(())
}
```

### Step-by-Step Breakdown

#### Step 1: Configuration

```rust
let config = Config::from_args();
```

Parse command-line arguments and create configuration.

#### Step 2: Logging Setup

```rust
let _subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .with_target(false)
    .with_thread_ids(false)
    .with_file(false)
    .with_line_number(false)
    .init();
```

The `tracing` ecosystem provides structured logging:

| Option | Effect |
|--------|--------|
| `with_max_level(Level::INFO)` | Log INFO and above (INFO, WARN, ERROR) |
| `with_target(false)` | Don't show module paths |
| `with_thread_ids(false)` | Don't show thread IDs |
| `with_file(false)` | Don't show source file names |
| `with_line_number(false)` | Don't show line numbers |

**Logging Levels:**
```rust
tracing::trace!("Very detailed information");
tracing::debug!("Debugging information");
tracing::info!("General information");
tracing::warn!("Warning");
tracing::error!("Error");
```

#### Step 3: Banner

```rust
print_banner(&config);
```

Displays a nice ASCII art banner when the server starts:

```
    _______ __           __    __ __  __    __
   / ____/ /___ _____ __/ /_  / //_/ | |  / /
  / /_  / / __ `/ __ `/ __ \/ ,<    | | / /
 / __/ / / /_/ / /_/ / / / / /| |   | |/ /
/_/   /_/\__,_/\__,_/_/ /_/_/ |_|   |___/

FlashKV v0.1.0 - High-Performance In-Memory Key-Value Database
──────────────────────────────────────────────────────────────
Server started on 127.0.0.1:6379
Ready to accept connections.
```

#### Step 4: Storage Engine

```rust
let storage = Arc::new(StorageEngine::new());
```

Creates the shared storage engine:
- `StorageEngine::new()` creates a new engine with 64 shards
- `Arc::new(...)` wraps it for thread-safe sharing

#### Step 5: Expiry Sweeper

```rust
let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
```

Starts the background task that cleans up expired keys:
- Runs in its own Tokio task
- Periodically scans shards for expired entries
- Uses adaptive intervals based on expiry rate

#### Step 6: Connection Statistics

```rust
let stats = Arc::new(ConnectionStats::new());
```

Shared statistics tracker for all connections:
- `connections_accepted` - Total connections
- `active_connections` - Currently connected clients
- `commands_processed` - Total commands executed
- `bytes_read` / `bytes_written` - Network I/O

#### Step 7: TCP Listener

```rust
let listener = TcpListener::bind(config.bind_address()).await?;
```

Binds the server to the configured address:
- Returns error if port is already in use
- `.await?` - Async operation that might fail

#### Step 8 & 9: Shutdown and Accept Loop

```rust
let shutdown = async {
    signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    info!("Shutdown signal received, stopping server...");
};

tokio::select! {
    _ = accept_loop(listener, storage, stats) => {}
    _ = shutdown => {}
}
```

This is the heart of the server - see [Graceful Shutdown](#graceful-shutdown) for details.

---

## Accept Loop

```rust
async fn accept_loop(
    listener: TcpListener,
    storage: Arc<StorageEngine>,
    stats: Arc<ConnectionStats>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // Create a command handler for this connection
                let handler = CommandHandler::new(Arc::clone(&storage));
                let stats = Arc::clone(&stats);

                // Spawn a task to handle this connection
                tokio::spawn(async move {
                    handle_connection(stream, addr, handler, stats).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}
```

### How It Works

```
┌────────────────────────────────────────────────────────────────────────────┐
│                           Accept Loop                                       │
│                                                                            │
│  ┌─────────────────────┐                                                   │
│  │                     │                                                   │
│  │  listener.accept()  │◄────── Waits for connection                      │
│  │                     │                                                   │
│  └──────────┬──────────┘                                                   │
│             │                                                              │
│             │ Connection arrives                                           │
│             ▼                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ For each connection:                                                 │  │
│  │                                                                      │  │
│  │  1. Clone Arc<StorageEngine>     ─── Cheap reference count bump     │  │
│  │  2. Clone Arc<ConnectionStats>   ─── Cheap reference count bump     │  │
│  │  3. Create CommandHandler        ─── New handler for this client    │  │
│  │  4. tokio::spawn(...)            ─── Spawn new task                 │  │
│  │                                                                      │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│             │                                                              │
│             │ Loop back immediately                                        │
│             ▼                                                              │
│       [Ready for next connection]                                          │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

### Key Points

1. **Non-blocking Accept**
   ```rust
   listener.accept().await
   ```
   This doesn't block the thread - it yields control to Tokio until a connection arrives.

2. **Per-Connection Task**
   ```rust
   tokio::spawn(async move {
       handle_connection(stream, addr, handler, stats).await;
   });
   ```
   Each connection runs in its own lightweight task. Tokio can handle thousands of these!

3. **Move Semantics**
   ```rust
   async move { ... }
   ```
   The `move` keyword transfers ownership of `stream`, `addr`, `handler`, and `stats` to the spawned task.

4. **Error Handling**
   ```rust
   Err(e) => {
       error!("Failed to accept connection: {}", e);
   }
   ```
   Log the error but keep running. One failed accept shouldn't crash the server.

### Concurrency Model

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Tokio Runtime                                   │
│                                                                         │
│  ┌───────────────────────┐                                             │
│  │    Accept Loop Task   │  ◄── Main task, spawns connection tasks     │
│  └───────────────────────┘                                             │
│              │                                                          │
│              │ spawns                                                   │
│              ▼                                                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐                   │
│  │Client 1  │ │Client 2  │ │Client 3  │ │Client N  │  ◄── Connection   │
│  │Task      │ │Task      │ │Task      │ │Task      │      tasks        │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘                   │
│       │            │            │            │                          │
│       └────────────┴────────────┴────────────┘                          │
│                         │                                               │
│                         ▼                                               │
│              ┌─────────────────────┐                                    │
│              │   StorageEngine     │  ◄── Shared, thread-safe           │
│              │   (Arc<...>)        │                                    │
│              └─────────────────────┘                                    │
│                                                                         │
│  ┌───────────────────────┐                                             │
│  │  Expiry Sweeper Task  │  ◄── Background task                        │
│  └───────────────────────┘                                             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Graceful Shutdown

### Using tokio::select!

```rust
tokio::select! {
    _ = accept_loop(listener, storage, stats) => {}
    _ = shutdown => {}
}
```

The `select!` macro races two futures:
1. `accept_loop` - Runs forever (until error)
2. `shutdown` - Completes when Ctrl+C is pressed

Whichever completes first "wins" and the other is dropped.

### The Shutdown Future

```rust
let shutdown = async {
    signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
    info!("Shutdown signal received, stopping server...");
};
```

- `signal::ctrl_c()` creates a future that completes on SIGINT (Ctrl+C)
- When completed, logs the shutdown message
- The `select!` then exits, allowing `main()` to return

### What Happens to Connections?

When the accept loop stops:
1. No new connections are accepted
2. Existing connection tasks continue running
3. When `main()` returns, the Tokio runtime shuts down
4. All spawned tasks are cancelled

For a more graceful shutdown (waiting for connections to finish), you could use a `tokio::sync::broadcast` channel to signal all tasks:

```rust
// More graceful shutdown (not implemented)
let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

// In accept loop, pass shutdown_rx to each connection
// Connections can then watch for shutdown signal and finish gracefully
```

---

## Data Flow

Here's how a request flows through the system:

```
┌───────────────────────────────────────────────────────────────────────────┐
│                              Request Flow                                  │
│                                                                           │
│  Client                                                                   │
│    │                                                                      │
│    │ TCP Connection                                                       │
│    ▼                                                                      │
│  ┌─────────────────────────────────────────────────────────────────────┐ │
│  │                         main.rs                                      │ │
│  │                                                                      │ │
│  │  listener.accept() ─────────┐                                       │ │
│  │                             │                                        │ │
│  │  tokio::spawn() ◄───────────┘                                       │ │
│  │       │                                                              │ │
│  └───────┼──────────────────────────────────────────────────────────────┘ │
│          │                                                                │
│          ▼                                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐ │
│  │                    connection/handler.rs                             │ │
│  │                                                                      │ │
│  │  1. Read bytes from socket                                          │ │
│  │  2. Parse RESP command (using protocol/parser.rs)                   │ │
│  │  3. Execute command (using commands/handler.rs)                     │ │
│  │  4. Access storage (using storage/engine.rs)                        │ │
│  │  5. Serialize response (using protocol/types.rs)                    │ │
│  │  6. Send bytes to socket                                            │ │
│  │                                                                      │ │
│  └─────────────────────────────────────────────────────────────────────┘ │
│          │                                                                │
│          ▼                                                                │
│  Client receives response                                                 │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

---

## Complete Code Reference

Here's the entire `main.rs` with annotations:

```rust
//! FlashKV - A High-Performance In-Memory Key-Value Database
//!
//! This is the main entry point for the FlashKV server.

use flashkv::commands::CommandHandler;
use flashkv::connection::{handle_connection, ConnectionStats};
use flashkv::storage::{start_expiry_sweeper, StorageEngine};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

/// Server configuration
struct Config {
    host: String,
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6379,
        }
    }
}

impl Config {
    fn from_args() -> Self {
        // ... argument parsing (see above)
    }

    fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn print_help() {
    println!(r#"
FlashKV - A High-Performance In-Memory Key-Value Database

USAGE:
    flashkv [OPTIONS]

OPTIONS:
    -h, --host <HOST>    Host to bind to (default: 127.0.0.1)
    -p, --port <PORT>    Port to listen on (default: 6379)
    -v, --version        Print version information
        --help           Print this help message
"#);
}

fn print_banner(config: &Config) {
    println!(r#"
    _______ __           __    __ __  __    __
   / ____/ /___ _____ __/ /_  / //_/ | |  / /
  / /_  / / __ `/ __ `/ __ \/ ,<    | | / /
 / __/ / / /_/ / /_/ / / / / /| |   | |/ /
/_/   /_/\__,_/\__,_/_/ /_/_/ |_|   |___/

FlashKV v{} - High-Performance In-Memory Key-Value Database
Server started on {}
"#, flashkv::VERSION, config.bind_address());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_args();
    
    // Initialize logging
    FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .init();

    print_banner(&config);

    // Create shared state
    let storage = Arc::new(StorageEngine::new());
    let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
    let stats = Arc::new(ConnectionStats::new());

    // Bind listener
    let listener = TcpListener::bind(config.bind_address()).await?;
    info!("Listening on {}", config.bind_address());

    // Run until shutdown
    let shutdown = async {
        signal::ctrl_c().await.ok();
        info!("Shutting down...");
    };

    tokio::select! {
        _ = accept_loop(listener, storage, stats) => {}
        _ = shutdown => {}
    }

    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    storage: Arc<StorageEngine>,
    stats: Arc<ConnectionStats>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let handler = CommandHandler::new(Arc::clone(&storage));
                let stats = Arc::clone(&stats);
                tokio::spawn(handle_connection(stream, addr, handler, stats));
            }
            Err(e) => error!("Accept error: {}", e),
        }
    }
}
```

---

## Exercises

### Exercise 1: Add a Connection Limit

Modify the server to limit the maximum number of concurrent connections:

```rust
// Hint: Use a semaphore
use tokio::sync::Semaphore;

const MAX_CONNECTIONS: usize = 1000;
let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

// In accept_loop, acquire permit before spawning
let permit = semaphore.clone().acquire_owned().await.ok();
tokio::spawn(async move {
    let _permit = permit; // Held for lifetime of connection
    handle_connection(stream, addr, handler, stats).await;
});
```

### Exercise 2: Add Metrics Endpoint

Add an HTTP endpoint (on a different port) that exposes server metrics:

```rust
// Hint: Use a simple HTTP parser or hyper
// GET /metrics returns:
// - connections_total
// - connections_active
// - commands_total
// - bytes_in_total
// - bytes_out_total
```

### Exercise 3: Add TLS Support

Add optional TLS encryption for client connections:

```rust
// Hint: Use tokio-rustls or tokio-native-tls
// Add --tls-cert and --tls-key command-line arguments
```

### Exercise 4: Add Authentication

Implement the AUTH command:

```rust
// Hint:
// 1. Add --requirepass <password> argument
// 2. Track authentication state per connection
// 3. Reject commands (except AUTH) if not authenticated
```

---

## Key Takeaways

1. **Initialization Order Matters**
   - Parse config first (might exit for --help)
   - Set up logging before anything else
   - Create storage before sweeper (sweeper needs Arc to storage)
   - Bind listener before starting accept loop

2. **Arc for Sharing**
   - `Arc<StorageEngine>` - Shared storage
   - `Arc<ConnectionStats>` - Shared stats
   - Clone Arc = increment reference count (cheap!)

3. **Tokio Task Per Connection**
   - Each connection is its own lightweight task
   - Tasks run concurrently, not in parallel (unless multi-threaded runtime)
   - Tokio scheduler multiplexes tasks onto threads

4. **Graceful Shutdown with select!**
   - Race multiple futures
   - First to complete wins
   - Others are dropped/cancelled

---

**Next:** [13_LIB_EXPORTS.md](./13_LIB_EXPORTS.md) - Understanding the library exports