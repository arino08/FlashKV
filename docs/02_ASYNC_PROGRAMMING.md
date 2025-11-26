# Async Programming with Tokio 🚀

This document explains async/await programming in Rust and how FlashKV uses Tokio to handle thousands of concurrent connections efficiently.

---

## Table of Contents

1. [The Problem: Blocking I/O](#1-the-problem-blocking-io)
2. [The Solution: Async I/O](#2-the-solution-async-io)
3. [Understanding Futures](#3-understanding-futures)
4. [The async/await Syntax](#4-the-asyncawait-syntax)
5. [Tokio Runtime](#5-tokio-runtime)
6. [Tasks and Spawning](#6-tasks-and-spawning)
7. [Async I/O Operations](#7-async-io-operations)
8. [Common Patterns in FlashKV](#8-common-patterns-in-flashkv)
9. [Exercises](#9-exercises)

---

## 1. The Problem: Blocking I/O

### Traditional Approach

Imagine a simple server that handles one client at a time:

```rust
fn handle_client(stream: TcpStream) {
    let mut buffer = [0u8; 1024];
    
    // This BLOCKS until data arrives
    stream.read(&mut buffer);  // ⏳ Waiting...
    
    // Process and respond
    stream.write(b"OK");
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();
    
    for stream in listener.incoming() {
        handle_client(stream.unwrap());  // One at a time!
    }
}
```

**Problem**: While waiting for one client's data, we can't serve anyone else!

### Thread-Per-Connection

One solution: spawn a thread for each client:

```rust
fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();
    
    for stream in listener.incoming() {
        let stream = stream.unwrap();
        std::thread::spawn(move || {
            handle_client(stream);
        });
    }
}
```

**Problems**:
- Threads are expensive (~2MB stack each)
- 10,000 clients = 20GB just for stacks!
- Context switching overhead is significant

---

## 2. The Solution: Async I/O

### How It Works

Instead of blocking when waiting for I/O, we:
1. Register interest in the I/O event
2. Do something else
3. Get notified when I/O is ready

```
Traditional (blocking):
Thread 1: [Read] ⏳⏳⏳⏳⏳ [Process] [Write] ⏳⏳⏳ [Read] ...

Async (non-blocking):
Task 1: [Read→] [Process] [Write→]
Task 2:    [Read→] [Process] [Write→]
Task 3:       [Read→] [Process] [Write→]
        ─────────────────────────────────────────>
                      Time
```

### The Key Insight

Most server time is spent WAITING for I/O:
- Waiting for network data to arrive
- Waiting for disk reads/writes
- Waiting for database responses

Async lets us use that waiting time productively!

---

## 3. Understanding Futures

### What Is a Future?

A `Future` represents a value that might not be ready yet:

```rust
trait Future {
    type Output;
    
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

enum Poll<T> {
    Ready(T),    // Value is ready!
    Pending,     // Not ready yet, will wake you later
}
```

### How Futures Work

1. You call `poll()` on a future
2. If the value is ready, it returns `Poll::Ready(value)`
3. If not ready, it returns `Poll::Pending` and arranges to wake you up later

```
        poll()           poll()           poll()
          │                │                │
          ▼                ▼                ▼
    ┌─────────┐      ┌─────────┐      ┌─────────┐
    │ Pending │ ──── │ Pending │ ──── │ Ready!  │
    └─────────┘      └─────────┘      └─────────┘
          │                │                │
          └────────────────┴────────────────┘
                  Time passing...
```

### You Don't Poll Manually!

The async runtime (Tokio) handles polling for you. You just write normal-looking code with `async/await`.

---

## 4. The async/await Syntax

### Basic Syntax

```rust
// An async function returns a Future
async fn fetch_data() -> String {
    // Do some async work...
    String::from("data")
}

// Use .await to wait for a Future
async fn process() {
    let data = fetch_data().await;  // Wait for fetch_data to complete
    println!("Got: {}", data);
}
```

### What async/await Actually Does

```rust
// This:
async fn greet() -> String {
    String::from("Hello")
}

// Is roughly equivalent to:
fn greet() -> impl Future<Output = String> {
    async {
        String::from("Hello")
    }
}
```

### Chaining Async Operations

```rust
async fn read_and_process(stream: &mut TcpStream) -> Result<String, Error> {
    // Each .await yields control if not ready
    let mut buffer = vec![0u8; 1024];
    
    let n = stream.read(&mut buffer).await?;  // Async read
    
    let parsed = parse(&buffer[..n]).await?;  // Async parse
    
    let result = transform(parsed).await?;    // Async transform
    
    Ok(result)
}
```

### Important: Futures Are Lazy!

Calling an async function does NOT run it:

```rust
async fn do_work() {
    println!("Working!");
}

fn main() {
    let future = do_work();  // Nothing happens yet!
    // future.await;         // Only when awaited does it run
}
```

---

## 5. Tokio Runtime

### What Is Tokio?

Tokio is an async runtime for Rust. It provides:
- A scheduler that runs async tasks
- Async I/O primitives (TCP, UDP, files)
- Timers and synchronization primitives
- Multi-threaded execution

### Creating a Tokio Runtime

```rust
// Option 1: The #[tokio::main] macro
#[tokio::main]
async fn main() {
    println!("Running in Tokio!");
}

// Option 2: Manual runtime creation
fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        println!("Running in Tokio!");
    });
}
```

### How FlashKV Uses It

```rust
// In main.rs
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Everything in here runs on the Tokio runtime
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    
    loop {
        let (stream, addr) = listener.accept().await?;
        // Spawn handlers...
    }
}
```

### Runtime Configuration

```rust
// Default: multi-threaded runtime
#[tokio::main]
async fn main() { }

// Single-threaded (for simpler debugging)
#[tokio::main(flavor = "current_thread")]
async fn main() { }

// Custom thread count
#[tokio::main(worker_threads = 4)]
async fn main() { }
```

---

## 6. Tasks and Spawning

### What Is a Task?

A task is like a lightweight thread. It's a unit of async work that can run concurrently with other tasks.

```
┌─────────────────────────────────────────────┐
│              Tokio Runtime                   │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐        │
│  │ Task 1  │ │ Task 2  │ │ Task 3  │  ...   │
│  │(Client1)│ │(Client2)│ │(Client3)│        │
│  └─────────┘ └─────────┘ └─────────┘        │
│       │           │           │              │
│       └───────────┼───────────┘              │
│                   ▼                          │
│         ┌─────────────────┐                  │
│         │  Thread Pool    │                  │
│         │ (e.g., 4 cores) │                  │
│         └─────────────────┘                  │
└─────────────────────────────────────────────┘
```

### Spawning Tasks

```rust
use tokio::task;

#[tokio::main]
async fn main() {
    // Spawn a task that runs concurrently
    let handle = tokio::spawn(async {
        // This runs independently
        do_work().await;
        42  // Return value
    });
    
    // Do other things while task runs...
    
    // Wait for the task and get its result
    let result = handle.await.unwrap();
    println!("Task returned: {}", result);
}
```

### How FlashKV Spawns Connection Handlers

```rust
// In main.rs
loop {
    let (stream, addr) = listener.accept().await?;
    
    let handler = CommandHandler::new(Arc::clone(&storage));
    let stats = Arc::clone(&stats);
    
    // Spawn a new task for this connection
    tokio::spawn(async move {
        handle_connection(stream, addr, handler, stats).await;
    });
    // Loop continues immediately - don't wait for handler!
}
```

### The `move` Keyword

`move` transfers ownership of captured variables into the async block:

```rust
let storage = Arc::clone(&storage);

// Without move: borrows storage (won't work - lifetime issues)
// tokio::spawn(async { use_storage(&storage) });

// With move: takes ownership of storage
tokio::spawn(async move {
    use_storage(&storage);  // storage moved into task
});
```

### Task vs Thread

| Feature | Thread | Task |
|---------|--------|------|
| Memory | ~2MB stack | ~few KB |
| Creation time | ~µs to ms | ~ns |
| Scheduling | OS kernel | User-space (Tokio) |
| Quantity | Hundreds | Millions |

---

## 7. Async I/O Operations

### Async TCP Listener

```rust
use tokio::net::TcpListener;

async fn run_server() {
    // Bind to address (async)
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();
    
    println!("Server listening...");
    
    loop {
        // Accept connection (async - yields while waiting)
        let (socket, addr) = listener.accept().await.unwrap();
        println!("New connection from {}", addr);
        
        // Handle connection...
    }
}
```

### Async Reading and Writing

```rust
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn handle_client(mut stream: TcpStream) {
    let mut buffer = vec![0u8; 1024];
    
    // Async read
    let n = stream.read(&mut buffer).await.unwrap();
    println!("Read {} bytes", n);
    
    // Async write
    stream.write_all(b"+PONG\r\n").await.unwrap();
    
    // Flush to ensure data is sent
    stream.flush().await.unwrap();
}
```

### Buffered I/O

```rust
use tokio::io::BufWriter;

// Wrap stream in a buffer for efficiency
let mut writer = BufWriter::new(stream);

// Multiple small writes are batched
writer.write_all(b"+").await?;
writer.write_all(b"OK").await?;
writer.write_all(b"\r\n").await?;

// Flush sends all buffered data
writer.flush().await?;
```

### How FlashKV Uses Buffered I/O

```rust
pub struct ConnectionHandler {
    // BufWriter buffers writes for efficiency
    stream: BufWriter<TcpStream>,
    
    // BytesMut for efficient read buffering
    buffer: BytesMut,
    // ...
}
```

---

## 8. Common Patterns in FlashKV

### Pattern 1: The Accept Loop

```rust
async fn accept_loop(listener: TcpListener, storage: Arc<StorageEngine>) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let storage = Arc::clone(&storage);
                
                // Spawn and forget - don't await the handler
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, addr, storage).await {
                        eprintln!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }
}
```

### Pattern 2: Read Loop with Buffer

```rust
async fn read_loop(stream: &mut TcpStream, buffer: &mut BytesMut) -> Result<()> {
    loop {
        // Try to parse from existing buffer
        if let Some(message) = try_parse(buffer)? {
            return Ok(message);
        }
        
        // Need more data - read from socket
        let n = stream.read_buf(buffer).await?;
        
        if n == 0 {
            // Connection closed
            return Err(Error::Disconnected);
        }
    }
}
```

### Pattern 3: Select for Multiple Futures

```rust
use tokio::select;
use tokio::signal;

async fn run_server() {
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();
    
    // Graceful shutdown handler
    let shutdown = async {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("Shutdown signal received");
    };
    
    // Run until either completes
    tokio::select! {
        _ = accept_loop(listener) => {
            println!("Accept loop ended");
        }
        _ = shutdown => {
            println!("Shutting down...");
        }
    }
}
```

### Pattern 4: Timeout

```rust
use tokio::time::{timeout, Duration};

async fn read_with_timeout(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; 1024];
    
    // Fail if read takes more than 5 seconds
    match timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
        Ok(Ok(n)) => Ok(buffer[..n].to_vec()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(Error::Timeout),
    }
}
```

### Pattern 5: Channels for Communication

```rust
use tokio::sync::mpsc;

async fn producer_consumer() {
    // Create a channel with buffer size 100
    let (tx, mut rx) = mpsc::channel(100);
    
    // Producer task
    tokio::spawn(async move {
        for i in 0..10 {
            tx.send(i).await.unwrap();
        }
    });
    
    // Consumer
    while let Some(value) = rx.recv().await {
        println!("Received: {}", value);
    }
}
```

### Pattern 6: Watch Channel for Shutdown

FlashKV uses this for the expiry sweeper:

```rust
use tokio::sync::watch;

async fn sweeper_with_shutdown() {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    
    // Sweeper task
    let sweeper = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    // Do cleanup work
                    cleanup_expired_keys();
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        println!("Sweeper shutting down");
                        return;
                    }
                }
            }
        }
    });
    
    // Later, signal shutdown
    shutdown_tx.send(true).unwrap();
    
    // Wait for sweeper to finish
    sweeper.await.unwrap();
}
```

---

## 9. Exercises

### Exercise 1: Basic Async Function

Write an async function `delayed_hello` that:
1. Waits for 1 second
2. Returns the string "Hello, async world!"

```rust
use tokio::time::{sleep, Duration};

async fn delayed_hello() -> String {
    // Your code here
}

#[tokio::main]
async fn main() {
    let message = delayed_hello().await;
    println!("{}", message);
}
```

### Exercise 2: Concurrent Tasks

Write a program that spawns 3 tasks:
- Task 1: prints "A" after 1 second
- Task 2: prints "B" after 2 seconds  
- Task 3: prints "C" after 3 seconds

All tasks should run concurrently (total time ~3 seconds, not 6).

### Exercise 3: Simple Echo Server

Create a TCP echo server that:
1. Listens on port 9999
2. Accepts multiple clients concurrently
3. Echoes back whatever each client sends

Test with: `nc localhost 9999`

### Exercise 4: Read with Timeout

Modify the echo server to disconnect clients who don't send anything for 10 seconds.

### Exercise 5: Graceful Shutdown

Add Ctrl+C handling to the echo server:
1. When Ctrl+C is pressed, stop accepting new connections
2. Print "Shutting down..."
3. Exit cleanly

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
use tokio::time::{sleep, Duration};

async fn delayed_hello() -> String {
    sleep(Duration::from_secs(1)).await;
    String::from("Hello, async world!")
}

#[tokio::main]
async fn main() {
    let message = delayed_hello().await;
    println!("{}", message);
}
```

### Exercise 2 Solution

```rust
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    let task_a = tokio::spawn(async {
        sleep(Duration::from_secs(1)).await;
        println!("A");
    });
    
    let task_b = tokio::spawn(async {
        sleep(Duration::from_secs(2)).await;
        println!("B");
    });
    
    let task_c = tokio::spawn(async {
        sleep(Duration::from_secs(3)).await;
        println!("C");
    });
    
    // Wait for all tasks
    let _ = tokio::join!(task_a, task_b, task_c);
}
```

### Exercise 3 Solution

```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    println!("Echo server listening on port 9999");
    
    loop {
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("New client: {}", addr);
        
        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];
            
            loop {
                match socket.read(&mut buffer).await {
                    Ok(0) => {
                        println!("Client {} disconnected", addr);
                        return;
                    }
                    Ok(n) => {
                        if socket.write_all(&buffer[..n]).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
    }
}
```

### Exercise 4 Solution

```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    println!("Echo server with timeout on port 9999");
    
    loop {
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("New client: {}", addr);
        
        tokio::spawn(async move {
            let mut buffer = [0u8; 1024];
            
            loop {
                let read_result = timeout(
                    Duration::from_secs(10),
                    socket.read(&mut buffer)
                ).await;
                
                match read_result {
                    Ok(Ok(0)) => {
                        println!("Client {} disconnected", addr);
                        return;
                    }
                    Ok(Ok(n)) => {
                        if socket.write_all(&buffer[..n]).await.is_err() {
                            return;
                        }
                    }
                    Ok(Err(_)) => return,
                    Err(_) => {
                        println!("Client {} timed out", addr);
                        return;
                    }
                }
            }
        });
    }
}
```

### Exercise 5 Solution

```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal;

async fn handle_client(mut socket: tokio::net::TcpStream, addr: std::net::SocketAddr) {
    let mut buffer = [0u8; 1024];
    
    loop {
        match socket.read(&mut buffer).await {
            Ok(0) => return,
            Ok(n) => {
                if socket.write_all(&buffer[..n]).await.is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

async fn accept_loop(listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                println!("New client: {}", addr);
                tokio::spawn(handle_client(socket, addr));
            }
            Err(e) => eprintln!("Accept error: {}", e),
        }
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    println!("Echo server on port 9999 (Ctrl+C to stop)");
    
    tokio::select! {
        _ = accept_loop(listener) => {}
        _ = signal::ctrl_c() => {
            println!("\nShutting down...");
        }
    }
}
```

</details>

---

## Key Takeaways

1. **Async is about efficiency** - Don't block, yield instead
2. **Futures are lazy** - They only run when awaited
3. **Tasks are cheap** - Spawn millions of them
4. **Tokio handles the complexity** - You just write async/await
5. **Move captures into spawned tasks** - They need ownership

---

## Next Steps

Now that you understand async programming, move on to:

**[03_NETWORKING_BASICS.md](./03_NETWORKING_BASICS.md)** - Learn about TCP, sockets, and how data flows over the network!