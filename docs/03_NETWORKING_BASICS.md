# Networking Basics for FlashKV 🌐

This document covers the networking concepts you need to understand how FlashKV accepts connections and communicates with clients over TCP.

---

## Table of Contents

1. [The OSI Model (Simplified)](#1-the-osi-model-simplified)
2. [TCP vs UDP](#2-tcp-vs-udp)
3. [Sockets and Ports](#3-sockets-and-ports)
4. [The TCP Connection Lifecycle](#4-the-tcp-connection-lifecycle)
5. [Buffering and Streaming](#5-buffering-and-streaming)
6. [Network Programming in Rust](#6-network-programming-in-rust)
7. [How FlashKV Handles Networking](#7-how-flashkv-handles-networking)
8. [Common Issues and Debugging](#8-common-issues-and-debugging)
9. [Exercises](#9-exercises)

---

## 1. The OSI Model (Simplified)

### The Layers That Matter for Us

```
┌─────────────────────────────────────────┐
│  Application Layer (HTTP, RESP, etc.)  │  ← You write this (FlashKV protocol)
├─────────────────────────────────────────┤
│  Transport Layer (TCP, UDP)            │  ← OS provides this
├─────────────────────────────────────────┤
│  Network Layer (IP)                    │  ← Router handles this
├─────────────────────────────────────────┤
│  Link Layer (Ethernet, WiFi)           │  ← Hardware handles this
└─────────────────────────────────────────┘
```

### What Each Layer Does

| Layer | Responsibility | Example |
|-------|----------------|---------|
| Application | Your protocol, data format | RESP, HTTP, JSON |
| Transport | Reliable delivery, ordering | TCP ensures all bytes arrive in order |
| Network | Routing between networks | IP addresses, routing tables |
| Link | Physical transmission | Ethernet frames, WiFi signals |

### FlashKV's Perspective

We work at the **Application Layer**:
- We receive raw bytes from TCP
- We parse them according to the RESP protocol
- We send response bytes back

The OS handles everything below!

---

## 2. TCP vs UDP

### TCP (Transmission Control Protocol)

**FlashKV uses TCP** because we need:

✅ **Reliability** - Every byte is guaranteed to arrive  
✅ **Ordering** - Bytes arrive in the order sent  
✅ **Connection-oriented** - Clear start and end of communication  
✅ **Flow control** - Sender slows down if receiver is overwhelmed  

```
Client                                Server
   │                                     │
   │──────── SYN ──────────────────────>│  "I want to connect"
   │<─────── SYN-ACK ───────────────────│  "OK, acknowledged"
   │──────── ACK ──────────────────────>│  "Connection established"
   │                                     │
   │←═══════ Data flows both ways ═════→│
   │                                     │
   │──────── FIN ──────────────────────>│  "I'm done"
   │<─────── ACK ───────────────────────│  "OK"
   │<─────── FIN ───────────────────────│  "Me too"
   │──────── ACK ──────────────────────>│  "Goodbye"
```

### UDP (User Datagram Protocol)

UDP is simpler but:

❌ No guaranteed delivery  
❌ No ordering  
❌ No connection state  
✅ Lower latency  
✅ Less overhead  

**Good for**: DNS, video streaming, games  
**Bad for**: Databases (we can't lose data!)

### Why Redis/FlashKV Uses TCP

```
Imagine if we used UDP:

Client: SET name "Alice"  ──── LOST ────X
Client: GET name
Server: (null)  ← Oops! SET was lost!
```

With TCP:
```
Client: SET name "Alice"  ──── Arrives & ACKed ────>
Client: GET name
Server: "Alice"  ← Correct!
```

---

## 3. Sockets and Ports

### What Is a Socket?

A socket is an endpoint for communication. Think of it as a "phone line" for network communication.

```
┌───────────────────────┐        ┌───────────────────────┐
│      Your App         │        │      Remote App       │
│  ┌─────────────────┐  │        │  ┌─────────────────┐  │
│  │     Socket      │◄─┼────────┼─►│     Socket      │  │
│  │  127.0.0.1:6379 │  │        │  │ 192.168.1.5:54321│  │
│  └─────────────────┘  │        │  └─────────────────┘  │
└───────────────────────┘        └───────────────────────┘
```

### What Is a Port?

A port is a 16-bit number (0-65535) that identifies a specific application on a host.

```
┌─────────────────────────────────────────────┐
│              Your Computer                   │
│                127.0.0.1                     │
│  ┌────────┐ ┌────────┐ ┌────────┐           │
│  │Port 80 │ │Port 443│ │Port 6379           │
│  │ HTTP   │ │ HTTPS  │ │ FlashKV│           │
│  └────────┘ └────────┘ └────────┘           │
└─────────────────────────────────────────────┘
```

### Special Port Numbers

| Range | Description | Examples |
|-------|-------------|----------|
| 0-1023 | Well-known (need root) | 80 (HTTP), 443 (HTTPS), 22 (SSH) |
| 1024-49151 | Registered | 3306 (MySQL), 5432 (PostgreSQL), 6379 (Redis) |
| 49152-65535 | Dynamic/Private | Client ephemeral ports |

### FlashKV Uses Port 6379

This is the same as Redis (for compatibility):

```rust
// Default binding
let listener = TcpListener::bind("127.0.0.1:6379").await?;
```

### IP Addresses

- `127.0.0.1` - Localhost (only accessible from this machine)
- `0.0.0.0` - All interfaces (accessible from other machines)
- `192.168.x.x` - Private network addresses

---

## 4. The TCP Connection Lifecycle

### Server Side

```rust
// 1. Create a socket and bind to an address
let listener = TcpListener::bind("127.0.0.1:6379").await?;

// 2. Listen for incoming connections (happens automatically)

// 3. Accept connections in a loop
loop {
    let (socket, addr) = listener.accept().await?;
    //   └─ New socket for this client
    //              └─ Client's address (IP:port)
    
    // 4. Handle the connection
    handle_client(socket).await;
}
```

### Client Side

```rust
// 1. Connect to the server
let socket = TcpStream::connect("127.0.0.1:6379").await?;

// 2. Send data
socket.write_all(b"PING\r\n").await?;

// 3. Receive response
let mut buf = [0u8; 1024];
let n = socket.read(&mut buf).await?;

// 4. Close (happens automatically when socket is dropped)
```

### The Three-Way Handshake

```
Client                          Server
   │                               │
   │ ──── SYN (seq=100) ────────> │  Client: "Let's talk, my seq is 100"
   │                               │
   │ <─── SYN-ACK (seq=300, ────── │  Server: "OK, my seq is 300, I got 100"
   │       ack=101)                │
   │                               │
   │ ──── ACK (ack=301) ────────> │  Client: "Got it, we're connected"
   │                               │
   │ ═══════ Connected! ═══════   │
```

### Connection Termination

```
Client                          Server
   │                               │
   │ ──── FIN ─────────────────> │  Client: "I'm done sending"
   │                               │
   │ <─── ACK ──────────────────  │  Server: "OK"
   │                               │
   │ <─── FIN ──────────────────  │  Server: "I'm done too"
   │                               │
   │ ──── ACK ─────────────────> │  Client: "Goodbye"
   │                               │
```

---

## 5. Buffering and Streaming

### The Core Challenge

TCP is a **stream** protocol - there are no message boundaries!

```
You send:       "SET key value\r\n" + "GET key\r\n"
TCP might deliver as:
  Read 1: "SET key val"
  Read 2: "ue\r\nGET key\r\n"

Or:
  Read 1: "SET key value\r\nGET key\r\n"

Or even:
  Read 1: "S"
  Read 2: "ET k"
  Read 3: "ey value\r\nGET key\r\n"
```

### Solution: Buffer and Parse

```rust
// Accumulate data in a buffer
let mut buffer = BytesMut::with_capacity(4096);

loop {
    // Try to parse a complete message
    if let Some(message) = try_parse(&buffer)? {
        // Remove parsed bytes from buffer
        buffer.advance(message.len());
        return Ok(message);
    }
    
    // Not enough data - read more
    let n = socket.read_buf(&mut buffer).await?;
    if n == 0 {
        return Err("Connection closed");
    }
}
```

### How FlashKV Does It

```rust
// In connection/handler.rs
pub struct ConnectionHandler {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,  // ← Accumulates incoming data
    parser: RespParser,
    // ...
}

async fn main_loop(&mut self) -> Result<(), ConnectionError> {
    loop {
        // Try parsing from buffer first
        while let Some(command) = self.try_parse_command()? {
            let response = self.command_handler.execute(command);
            self.send_response(&response).await?;
        }
        
        // Need more data
        self.read_more_data().await?;
    }
}
```

### Read Buffer States

```
State 1: Buffer has partial message
┌────────────────────────────────────────┐
│ * 3 \r \n $ 3 \r \n S E T \r \n $ 3 \r│ ← Incomplete!
└────────────────────────────────────────┘
                                       ↑
                                 Need more data

State 2: After reading more
┌────────────────────────────────────────────────────────────┐
│ * 3 \r \n $ 3 \r \n S E T \r \n $ 3 \r \n k e y \r \n ... │
└────────────────────────────────────────────────────────────┘
                                                           ↑
                                              Now we can parse!

State 3: After parsing, buffer contains next message
┌────────────────────────────────────────┐
│ * 2 \r \n $ 3 \r \n G E T \r \n ...   │ ← Next command ready
└────────────────────────────────────────┘
```

---

## 6. Network Programming in Rust

### Standard Library (Blocking)

```rust
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();
    
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();  // BLOCKS!
        
        stream.write_all(b"+PONG\r\n").unwrap();
    }
}
```

### Tokio (Async)

```rust
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();
    
    loop {
        let (mut stream, _) = listener.accept().await.unwrap();
        
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();  // Yields, doesn't block!
            
            stream.write_all(b"+PONG\r\n").await.unwrap();
        });
    }
}
```

### Key Differences

| Feature | std::net | tokio::net |
|---------|----------|------------|
| Read/Write | Blocks thread | Yields to runtime |
| Concurrency | 1 thread per connection | Thousands per thread |
| Traits | `Read`, `Write` | `AsyncRead`, `AsyncWrite` |
| Usage | Simple tools | Production servers |

### BytesMut for Efficient Buffering

```rust
use bytes::BytesMut;

let mut buf = BytesMut::with_capacity(4096);

// Read into buffer (grows if needed)
stream.read_buf(&mut buf).await?;

// Access data
let data: &[u8] = &buf[..];

// Split off processed data (zero-copy!)
let message = buf.split_to(n);

// Remaining data stays in buf
```

---

## 7. How FlashKV Handles Networking

### The Server Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              main.rs                                     │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                      TcpListener                                 │    │
│  │                    127.0.0.1:6379                                │    │
│  └────────────────────────────┬────────────────────────────────────┘    │
│                               │                                          │
│                               │ accept()                                 │
│                               ▼                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                        For each client...                          │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                │  │
│  │  │  Task 1     │  │  Task 2     │  │  Task N     │                │  │
│  │  │ (Client 1)  │  │ (Client 2)  │  │ (Client N)  │                │  │
│  │  │  TcpStream  │  │  TcpStream  │  │  TcpStream  │                │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘                │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### Connection Flow

```rust
// 1. Server starts and binds
let listener = TcpListener::bind(config.bind_address()).await?;

// 2. Accept loop
loop {
    let (stream, addr) = listener.accept().await?;
    
    // 3. Spawn a handler task for this connection
    let handler = CommandHandler::new(Arc::clone(&storage));
    let stats = Arc::clone(&stats);
    
    tokio::spawn(async move {
        handle_connection(stream, addr, handler, stats).await;
    });
}
```

### Per-Connection Handler

```rust
// In connection/handler.rs
pub struct ConnectionHandler {
    stream: BufWriter<TcpStream>,   // Buffered writes
    addr: SocketAddr,               // Client address
    buffer: BytesMut,               // Read buffer
    command_handler: CommandHandler,
    parser: RespParser,
    stats: Arc<ConnectionStats>,
}

impl ConnectionHandler {
    pub async fn run(mut self) -> Result<(), ConnectionError> {
        loop {
            // 1. Try to parse complete commands from buffer
            while let Some(command) = self.try_parse_command()? {
                // 2. Execute command
                let response = self.command_handler.execute(command);
                
                // 3. Send response
                self.send_response(&response).await?;
            }
            
            // 4. Read more data into buffer
            self.read_more_data().await?;
        }
    }
}
```

### Reading Data

```rust
async fn read_more_data(&mut self) -> Result<(), ConnectionError> {
    // Check buffer limit
    if self.buffer.len() >= MAX_BUFFER_SIZE {
        return Err(ConnectionError::BufferFull);
    }
    
    // Read from socket into buffer
    let n = self.stream.get_mut().read_buf(&mut self.buffer).await?;
    
    // 0 bytes = connection closed
    if n == 0 {
        return Err(ConnectionError::ClientDisconnected);
    }
    
    self.stats.bytes_read(n);
    Ok(())
}
```

### Sending Responses

```rust
async fn send_response(&mut self, response: &RespValue) -> Result<(), ConnectionError> {
    // Serialize to bytes
    let bytes = response.serialize();
    
    // Write to buffered stream
    self.stream.write_all(&bytes).await?;
    
    // Flush to ensure data is sent
    self.stream.flush().await?;
    
    self.stats.bytes_written(bytes.len());
    Ok(())
}
```

---

## 8. Common Issues and Debugging

### Issue 1: "Address already in use"

```
Error: Os { code: 98, message: "Address already in use" }
```

**Cause**: Another process is using the port.

**Fix**:
```bash
# Find what's using the port
lsof -i :6379

# Kill it (or use a different port)
kill <PID>
```

### Issue 2: Connection Refused

```
Error: Connection refused (os error 111)
```

**Cause**: No server listening on that address.

**Check**:
- Is the server running?
- Correct IP and port?
- Firewall blocking?

### Issue 3: Partial Reads

Your code reads less than expected:

```rust
let mut buf = [0u8; 1024];
let n = stream.read(&mut buf).await?;
// n might be less than 1024!
```

**Solution**: Loop until you have enough data:

```rust
async fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<()> {
    let mut total = 0;
    while total < buf.len() {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            return Err("Unexpected EOF");
        }
        total += n;
    }
    Ok(())
}
```

### Issue 4: Write Doesn't Send Immediately

```rust
stream.write_all(b"data").await?;
// Data might still be buffered!
```

**Solution**: Always flush:

```rust
stream.write_all(b"data").await?;
stream.flush().await?;  // Now it's sent
```

### Debugging Tools

```bash
# Test with netcat
nc localhost 6379
# Type: PING
# Should see: PONG

# Monitor network traffic
tcpdump -i lo port 6379

# Check open connections
ss -tuln | grep 6379
netstat -an | grep 6379
```

---

## 9. Exercises

### Exercise 1: Simple TCP Client

Write a program that:
1. Connects to `localhost:6379`
2. Sends `*1\r\n$4\r\nPING\r\n`
3. Reads and prints the response

### Exercise 2: Concurrent Connections

Modify the client to:
1. Open 100 connections simultaneously
2. Send PING on each
3. Measure total time

### Exercise 3: Echo Server

Build a TCP echo server that:
1. Accepts multiple clients
2. Echoes back each line (ending with `\n`)
3. Handles partial reads correctly

### Exercise 4: Line-Based Protocol

Create a simple protocol:
- Commands: `HELLO`, `TIME`, `QUIT`
- Each command ends with `\n`
- Server responds with appropriate message

### Exercise 5: Connection Pooling

Build a client that:
1. Maintains a pool of 10 connections
2. Reuses connections for multiple requests
3. Handles connection failures gracefully

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:6379").await.unwrap();
    
    // Send PING command in RESP format
    stream.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
    
    // Read response
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).await.unwrap();
    
    println!("Response: {}", String::from_utf8_lossy(&buf[..n]));
}
```

### Exercise 2 Solution

```rust
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Instant;

async fn ping_once() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect("127.0.0.1:6379").await?;
    stream.write_all(b"*1\r\n$4\r\nPING\r\n").await?;
    
    let mut buf = [0u8; 64];
    let _ = stream.read(&mut buf).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let start = Instant::now();
    
    let mut handles = vec![];
    for _ in 0..100 {
        handles.push(tokio::spawn(ping_once()));
    }
    
    for handle in handles {
        handle.await.unwrap().unwrap();
    }
    
    println!("100 connections completed in {:?}", start.elapsed());
}
```

### Exercise 3 Solution

```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    println!("Echo server on port 9999");
    
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        println!("Client connected: {}", addr);
        
        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,  // EOF
                    Ok(_) => {
                        writer.write_all(line.as_bytes()).await.unwrap();
                    }
                    Err(_) => break,
                }
            }
            
            println!("Client disconnected: {}", addr);
        });
    }
}
```

### Exercise 4 Solution

```rust
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use chrono::Utc;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9999").await.unwrap();
    println!("Command server on port 9999");
    
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        
        tokio::spawn(async move {
            let (reader, mut writer) = socket.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                
                let response = match line.trim().to_uppercase().as_str() {
                    "HELLO" => format!("Hello, {}!\n", addr),
                    "TIME" => format!("{}\n", Utc::now()),
                    "QUIT" => {
                        writer.write_all(b"Goodbye!\n").await.unwrap();
                        break;
                    }
                    _ => "Unknown command\n".to_string(),
                };
                
                writer.write_all(response.as_bytes()).await.unwrap();
            }
        });
    }
}
```

### Exercise 5 Solution

```rust
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::VecDeque;

struct ConnectionPool {
    connections: Mutex<VecDeque<TcpStream>>,
    address: String,
    max_size: usize,
}

impl ConnectionPool {
    fn new(address: &str, max_size: usize) -> Self {
        Self {
            connections: Mutex::new(VecDeque::new()),
            address: address.to_string(),
            max_size,
        }
    }
    
    async fn get(&self) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
        let mut pool = self.connections.lock().await;
        
        if let Some(conn) = pool.pop_front() {
            Ok(conn)
        } else {
            Ok(TcpStream::connect(&self.address).await?)
        }
    }
    
    async fn put(&self, conn: TcpStream) {
        let mut pool = self.connections.lock().await;
        if pool.len() < self.max_size {
            pool.push_back(conn);
        }
        // else: connection is dropped
    }
}

#[tokio::main]
async fn main() {
    let pool = Arc::new(ConnectionPool::new("127.0.0.1:6379", 10));
    
    let mut handles = vec![];
    for i in 0..50 {
        let pool = Arc::clone(&pool);
        handles.push(tokio::spawn(async move {
            let mut conn = pool.get().await.unwrap();
            
            conn.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
            
            let mut buf = [0u8; 64];
            conn.read(&mut buf).await.unwrap();
            
            pool.put(conn).await;
            println!("Request {} done", i);
        }));
    }
    
    for handle in handles {
        handle.await.unwrap();
    }
}
```

</details>

---

## Key Takeaways

1. **TCP is a stream** - No message boundaries, you must frame your protocol
2. **Buffering is essential** - Always accumulate data before parsing
3. **Async I/O scales** - Handle thousands of connections per thread
4. **Flush your writes** - Data might be buffered until you flush
5. **Handle partial reads** - You might not get all data in one read

---

## Next Steps

Now that you understand networking, move on to:

**[04_RESP_PROTOCOL.md](./04_RESP_PROTOCOL.md)** - Learn the Redis protocol that FlashKV speaks!