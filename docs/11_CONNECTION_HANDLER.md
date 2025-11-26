# 11. Connection Handler Deep Dive 🔌

**Time:** 45 minutes  
**File:** `src/connection/handler.rs`

This document explains how FlashKV manages individual client connections, including buffer management, the read-execute-respond loop, and error handling.

---

## Table of Contents

1. [Overview](#overview)
2. [Connection Lifecycle](#connection-lifecycle)
3. [The ConnectionHandler Struct](#the-connectionhandler-struct)
4. [Buffer Management](#buffer-management)
5. [The Main Loop](#the-main-loop)
6. [Parsing Commands](#parsing-commands)
7. [Sending Responses](#sending-responses)
8. [Connection Statistics](#connection-statistics)
9. [Error Handling](#error-handling)
10. [Pipelining Support](#pipelining-support)
11. [Key Takeaways](#key-takeaways)
12. [Exercises](#exercises)

---

## Overview

The connection handler is responsible for:

1. **Managing TCP streams** - Reading bytes from and writing bytes to the socket
2. **Buffering data** - Accumulating partial reads until complete commands are available
3. **Parsing commands** - Using the RESP parser to extract commands
4. **Executing commands** - Delegating to the command handler
5. **Sending responses** - Serializing and writing responses back
6. **Tracking statistics** - Monitoring connections, commands, and bytes

Each client connection gets its own `ConnectionHandler` instance running in a separate Tokio task.

---

## Connection Lifecycle

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         Connection Lifecycle                             │
│                                                                          │
│   1. TCP Connection Established                                          │
│      │                                                                   │
│      ▼                                                                   │
│   2. tokio::spawn() creates new task                                     │
│      │                                                                   │
│      ▼                                                                   │
│   3. ConnectionHandler::new() called                                     │
│      • Wrap stream in BufWriter                                          │
│      • Allocate read buffer                                              │
│      • Increment active connections                                      │
│      │                                                                   │
│      ▼                                                                   │
│   4. ConnectionHandler::run() starts main loop                           │
│      │                                                                   │
│      ▼                                                                   │
│   5. ┌──────────────────────────────────────────────────────────────┐    │
│      │                    Main Loop (repeats)                        │   │
│      │                                                               │   │
│      │    ┌──────────────────────┐                                   │   │
│      │    │ Try parse command    │◄──────────────────────────┐       │   │
│      │    └──────────┬───────────┘                           │       │   │
│      │               │                                       │       │   │
│      │      ┌────────┴────────┐                              │       │   │
│      │      │                 │                              │       │   │
│      │      ▼                 ▼                              │       │   │
│      │  [Complete]      [Incomplete]                         │       │   │
│      │      │                 │                              │       │   │
│      │      ▼                 ▼                              │       │   │
│      │  ┌─────────┐    ┌──────────────┐                      │       │   │
│      │  │ Execute │    │ Read more    │                      │       │   │
│      │  │ command │    │ data from    │──────────────────────┘       │   │
│      │  └────┬────┘    │ socket       │                              │   │
│      │       │         └──────────────┘                              │   │
│      │       ▼                                                       │   │
│      │  ┌─────────────────┐                                          │   │
│      │  │ Send response   │                                          │   │
│      │  └────────┬────────┘                                          │   │
│      │           │                                                   │   │
│      │           └───────────────────────────────────────────────────┘   │
│      └───────────────────────────────────────────────────────────────┘   │
│      │                                                                   │
│      ▼                                                                   │
│   6. Connection ends (client disconnect / error)                         │
│      │                                                                   │
│      ▼                                                                   │
│   7. Stats updated, task completes                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## The ConnectionHandler Struct

```rust
pub struct ConnectionHandler {
    /// The TCP stream for this connection
    stream: BufWriter<TcpStream>,

    /// Client's address (for logging)
    addr: SocketAddr,

    /// Buffer for incoming data
    buffer: BytesMut,

    /// The command handler (shared across connections)
    command_handler: CommandHandler,

    /// RESP parser
    parser: RespParser,

    /// Connection statistics (shared)
    stats: Arc<ConnectionStats>,
}
```

### Field Breakdown

| Field | Type | Purpose |
|-------|------|---------|
| `stream` | `BufWriter<TcpStream>` | Buffered TCP stream for efficient writes |
| `addr` | `SocketAddr` | Client's IP:port for logging |
| `buffer` | `BytesMut` | Accumulator for incoming bytes |
| `command_handler` | `CommandHandler` | Executes Redis commands |
| `parser` | `RespParser` | Parses RESP protocol |
| `stats` | `Arc<ConnectionStats>` | Shared statistics counters |

### Why BufWriter?

Wrapping the `TcpStream` in a `BufWriter` provides:

1. **Fewer syscalls** - Small writes are batched together
2. **Better performance** - Reduces overhead of per-write syscalls
3. **Explicit control** - We call `flush()` when we want data sent

```rust
// Without BufWriter: 3 syscalls
stream.write_all(b"+OK").await?;
stream.write_all(b"\r\n").await?;
// Each write_all is a syscall!

// With BufWriter: 1 syscall
writer.write_all(b"+OK").await?;
writer.write_all(b"\r\n").await?;
writer.flush().await?;  // Single syscall sends all buffered data
```

---

## Buffer Management

### The Challenge: TCP is a Stream Protocol

TCP doesn't preserve message boundaries. When a client sends:

```
*1\r\n$4\r\nPING\r\n*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n
```

We might receive it as:

- **Read 1:** `*1\r\n$4\r\nPI`
- **Read 2:** `NG\r\n*3\r\n$3\r\nSE`
- **Read 3:** `T\r\n$3\r\nkey\r\n$5\r\nvalue\r\n`

We need to:
1. Buffer partial data until a complete command is available
2. Handle multiple commands in a single read (pipelining)
3. Prevent unbounded buffer growth (memory safety)

### Buffer Constants

```rust
/// Maximum size for the read buffer (64 KB)
const MAX_BUFFER_SIZE: usize = 64 * 1024;

/// Initial buffer capacity
const INITIAL_BUFFER_SIZE: usize = 4096;
```

- **MAX_BUFFER_SIZE**: Prevents malicious clients from exhausting memory
- **INITIAL_BUFFER_SIZE**: Good starting size for most commands

### Buffer Growth Strategy

```rust
async fn read_more_data(&mut self) -> Result<(), ConnectionError> {
    // Check buffer size limit
    if self.buffer.len() >= MAX_BUFFER_SIZE {
        return Err(ConnectionError::BufferFull);
    }

    // Ensure we have some capacity
    if self.buffer.capacity() - self.buffer.len() < 1024 {
        self.buffer.reserve(4096);
    }

    // Read data
    let n = self.stream.get_mut().read_buf(&mut self.buffer).await?;

    if n == 0 {
        // Connection closed by client
        if self.buffer.is_empty() {
            return Err(ConnectionError::ClientDisconnected);
        } else {
            return Err(ConnectionError::UnexpectedEof);
        }
    }

    Ok(())
}
```

**Key Points:**

1. **Size limit check** - Reject if buffer exceeds 64KB
2. **Lazy reservation** - Only reserve more space when needed
3. **Zero-byte detection** - `n == 0` means client closed connection
4. **Partial data handling** - Error if data remains but connection closed

### BytesMut Operations

`BytesMut` provides efficient buffer operations:

```rust
// Reserve capacity (may reallocate)
buffer.reserve(4096);

// Reading appends to end
stream.read_buf(&mut buffer).await?;

// After parsing, consume bytes from front
let _ = buffer.split_to(consumed);
```

**Visual representation:**

```text
Before split_to(10):
┌──────────────────────────────────────┐
│ P A R S E D │ R E M A I N I N G      │
└──────────────────────────────────────┘
  ←── 10 ───→  ←────── rest ─────────→

After split_to(10):
┌──────────────────────────────────────┐
│ R E M A I N I N G                    │
└──────────────────────────────────────┘
  ←────────── all ──────────────────→
```

---

## The Main Loop

```rust
async fn main_loop(&mut self) -> Result<(), ConnectionError> {
    loop {
        // Try to parse a complete command from the buffer
        while let Some(command) = self.try_parse_command()? {
            // Execute the command
            let response = self.command_handler.execute(command);
            self.stats.command_processed();

            // Send the response
            self.send_response(&response).await?;
        }

        // Need more data - read from the socket
        self.read_more_data().await?;
    }
}
```

### Understanding the Double Loop

There are **two loops** here:

1. **Outer `loop`** - Runs until error or disconnect
2. **Inner `while let`** - Processes all complete commands in buffer

This design supports **pipelining** - when multiple commands arrive in one TCP read.

### Execution Flow

```text
Buffer: "*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n"

Iteration 1:
├─ Inner while: Parse PING → Execute → Send +PONG\r\n
├─ Inner while: Parse PING → Execute → Send +PONG\r\n
├─ Inner while: try_parse returns None (buffer empty)
└─ Read more data...

Buffer: "*3\r\n$3\r\n" (incomplete SET command)

Iteration 2:
├─ Inner while: try_parse returns None (incomplete)
└─ Read more data...

Buffer: "*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"

Iteration 3:
├─ Inner while: Parse SET → Execute → Send +OK\r\n
├─ Inner while: try_parse returns None
└─ Read more data...
```

---

## Parsing Commands

```rust
fn try_parse_command(&mut self) -> Result<Option<RespValue>, ConnectionError> {
    if self.buffer.is_empty() {
        return Ok(None);
    }

    match self.parser.parse(&self.buffer) {
        Ok(Some((value, consumed))) => {
            // Successfully parsed - consume the bytes
            let _ = self.buffer.split_to(consumed);
            Ok(Some(value))
        }
        Ok(None) => {
            // Incomplete - need more data
            Ok(None)
        }
        Err(e) => {
            // Parse error
            Err(ConnectionError::ParseError(e))
        }
    }
}
```

### Three Outcomes

| Result | Meaning | Action |
|--------|---------|--------|
| `Ok(Some((value, consumed)))` | Complete command parsed | Execute it, remove bytes from buffer |
| `Ok(None)` | Incomplete data | Read more from socket |
| `Err(e)` | Invalid RESP | Close connection with error |

### Buffer Consumption

After successful parsing, we must consume exactly the right number of bytes:

```rust
let _ = self.buffer.split_to(consumed);
```

This is critical because:
- Too few bytes: Next parse sees garbage
- Too many bytes: We lose data from next command
- The parser tells us exactly how many bytes it consumed

---

## Sending Responses

```rust
async fn send_response(&mut self, response: &RespValue) -> Result<(), ConnectionError> {
    let bytes = response.serialize();
    self.stream.write_all(&bytes).await?;
    self.stream.flush().await?;
    self.stats.bytes_written(bytes.len());
    Ok(())
}
```

### Why flush()?

Without `flush()`, data may sit in the `BufWriter` buffer:

```rust
// This might not send immediately!
writer.write_all(b"+PONG\r\n").await?;

// This ensures it's sent now
writer.flush().await?;
```

For a key-value store, low latency is critical. We flush after each response to minimize latency.

### Potential Optimization: Batch Flushing

For even better performance with pipelining, you could batch responses:

```rust
// Instead of flushing per response...
while let Some(command) = self.try_parse_command()? {
    let response = self.command_handler.execute(command);
    self.stream.write_all(&response.serialize()).await?;
    // Don't flush yet!
}
// Flush once after all responses
self.stream.flush().await?;
```

This trades latency for throughput in high-pipeline scenarios.

---

## Connection Statistics

```rust
#[derive(Debug, Default)]
pub struct ConnectionStats {
    pub connections_accepted: AtomicU64,
    pub active_connections: AtomicU64,
    pub commands_processed: AtomicU64,
    pub bytes_read: AtomicU64,
    pub bytes_written: AtomicU64,
}
```

### Why Atomics?

Statistics are shared across all connection handlers:

```text
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Connection 1   │     │  Connection 2   │     │  Connection 3   │
│  (Tokio Task)   │     │  (Tokio Task)   │     │  (Tokio Task)   │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         │                       ▼                       │
         │              ┌─────────────────┐              │
         └─────────────►│ ConnectionStats │◄─────────────┘
                        │   (Atomics)     │
                        └─────────────────┘
```

Atomics allow lock-free concurrent updates:

```rust
impl ConnectionStats {
    pub fn connection_opened(&self) {
        self.connections_accepted.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn command_processed(&self) {
        self.commands_processed.fetch_add(1, Ordering::Relaxed);
    }
}
```

### Why Relaxed Ordering?

`Ordering::Relaxed` is the fastest ordering. It's sufficient here because:
- Statistics don't need to be perfectly synchronized
- We only care about eventual accuracy, not exact real-time values
- There are no dependent operations that require ordering guarantees

---

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),

    #[error("Client disconnected")]
    ClientDisconnected,

    #[error("Unexpected end of stream")]
    UnexpectedEof,

    #[error("Buffer size limit exceeded")]
    BufferFull,
}
```

### Error Categories

| Error | Cause | Response |
|-------|-------|----------|
| `IoError` | Network issues, syscall failures | Log and close connection |
| `ParseError` | Invalid RESP data | Close connection |
| `ClientDisconnected` | Client closed socket cleanly | Normal termination |
| `UnexpectedEof` | Socket closed mid-command | Log warning, close |
| `BufferFull` | Client sent >64KB without complete command | Close connection |

### Graceful Error Handling

```rust
pub async fn run(mut self) -> Result<(), ConnectionError> {
    let result = self.main_loop().await;

    match &result {
        Ok(()) => info!("Client disconnected gracefully"),
        Err(e) => match e {
            ConnectionError::ClientDisconnected => {
                debug!("Client disconnected")  // Normal, quiet
            }
            ConnectionError::IoError(io_err)
                if io_err.kind() == std::io::ErrorKind::ConnectionReset =>
            {
                debug!("Connection reset by client")  // Common, quiet
            }
            _ => warn!("Connection error: {}", e),  // Unusual, log warning
        },
    }

    self.stats.connection_closed();
    result
}
```

This logging strategy:
- **Quiet for normal disconnects** - Don't spam logs
- **Debug level for common network events** - Connection resets happen
- **Warning level for unusual errors** - Parse errors, buffer full

---

## Pipelining Support

**Pipelining** is when a client sends multiple commands without waiting for responses:

```text
Client sends (without waiting):
*1\r\n$4\r\nPING\r\n
*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n
*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n

Server responds (in order):
+PONG\r\n
+OK\r\n
$3\r\nbar\r\n
```

### Why Pipelining Matters

Without pipelining:
```text
RTT = 1ms
Commands = 100
Total time = 100 * 1ms = 100ms
```

With pipelining:
```text
RTT = 1ms
Commands = 100
Total time ≈ 1ms (all commands in one round trip!)
```

### How We Support It

Our `while let` loop naturally handles pipelining:

```rust
// Parse ALL complete commands before reading more
while let Some(command) = self.try_parse_command()? {
    let response = self.command_handler.execute(command);
    self.send_response(&response).await?;
}
```

If the buffer contains 10 complete commands, we execute all 10 before the next `read_more_data()`.

---

## The Public API

```rust
/// Convenience function to handle a connection
pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    command_handler: CommandHandler,
    stats: Arc<ConnectionStats>,
) {
    let handler = ConnectionHandler::new(stream, addr, command_handler, stats);
    if let Err(e) = handler.run().await {
        // Handle specific error types quietly
        match e {
            ConnectionError::ClientDisconnected => {}
            ConnectionError::IoError(ref io_err)
                if io_err.kind() == std::io::ErrorKind::ConnectionReset => {}
            _ => debug!("Connection ended with error: {}", e),
        }
    }
}
```

This is the entry point called from `main.rs`:

```rust
tokio::spawn(async move {
    handle_connection(stream, addr, handler, stats).await;
});
```

---

## Key Takeaways

### 1. TCP is a Stream Protocol
You must buffer and parse incrementally. Never assume one read = one command.

### 2. Resource Limits are Essential
The 64KB buffer limit prevents memory exhaustion from malicious clients.

### 3. Pipelining is Free
The read-parse-execute loop naturally supports pipelining without special code.

### 4. BufWriter Improves Performance
Batching writes reduces syscall overhead significantly.

### 5. Atomics Enable Lock-Free Stats
Using atomics for counters avoids mutex contention across connections.

### 6. Error Handling Should Be Granular
Different errors warrant different log levels and responses.

---

## Exercises

### Exercise 1: Add Connection Timeout

Add a timeout that disconnects clients who don't send data for 60 seconds:

```rust
// Hint: Use tokio::time::timeout
async fn read_more_data(&mut self) -> Result<(), ConnectionError> {
    use tokio::time::{timeout, Duration};
    
    let read_future = self.stream.get_mut().read_buf(&mut self.buffer);
    match timeout(Duration::from_secs(60), read_future).await {
        Ok(Ok(n)) => { /* normal handling */ }
        Ok(Err(e)) => return Err(ConnectionError::IoError(e)),
        Err(_) => return Err(ConnectionError::Timeout),
    }
}
```

### Exercise 2: Rate Limiting

Add per-connection rate limiting (max 1000 commands/second):

```rust
struct ConnectionHandler {
    // Add these fields:
    commands_this_second: u32,
    second_start: Instant,
}
```

### Exercise 3: Command Logging

Add optional command logging for debugging:

```rust
// Log commands when TRACE level is enabled
trace!(
    client = %self.addr,
    command = ?command,
    "Executing command"
);
```

### Exercise 4: Graceful Shutdown

Modify the handler to support graceful shutdown when the server is stopping:

```rust
pub async fn run(mut self, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
    loop {
        tokio::select! {
            result = self.main_loop_step() => {
                if let Err(e) = result {
                    break;
                }
            }
            _ = shutdown.recv() => {
                info!("Shutting down connection");
                break;
            }
        }
    }
}
```

---

**Next:** [12_MAIN_SERVER.md](./12_MAIN_SERVER.md) - Understanding the server entry point