# RESP Protocol - The Language FlashKV Speaks 📡

This document explains the Redis Serialization Protocol (RESP), which is how clients communicate with FlashKV.

---

## Table of Contents

1. [What Is RESP?](#1-what-is-resp)
2. [RESP Data Types](#2-resp-data-types)
3. [Type Prefixes](#3-type-prefixes)
4. [Simple Strings](#4-simple-strings)
5. [Errors](#5-errors)
6. [Integers](#6-integers)
7. [Bulk Strings](#7-bulk-strings)
8. [Arrays](#8-arrays)
9. [Commands as Arrays](#9-commands-as-arrays)
10. [Parsing Strategy](#10-parsing-strategy)
11. [Exercises](#11-exercises)

---

## 1. What Is RESP?

### Definition

RESP (REdis Serialization Protocol) is a simple, binary-safe protocol used for client-server communication in Redis (and FlashKV).

### Why RESP?

| Feature | Benefit |
|---------|---------|
| Simple | Easy to implement (we did it!) |
| Fast to parse | Minimal CPU overhead |
| Binary-safe | Can store any bytes, not just text |
| Human-readable | Easy to debug with telnet/netcat |

### Binary-Safe?

"Binary-safe" means you can store ANY bytes, including:
- Null bytes (`\0`)
- Raw image data
- Compressed data
- Encrypted data

This is possible because RESP uses length prefixes, not terminators.

---

## 2. RESP Data Types

RESP has 5 data types:

| Type | Prefix | Example | Use Case |
|------|--------|---------|----------|
| Simple String | `+` | `+OK\r\n` | Short status messages |
| Error | `-` | `-ERR unknown\r\n` | Error messages |
| Integer | `:` | `:1000\r\n` | Numeric responses |
| Bulk String | `$` | `$5\r\nhello\r\n` | Binary data |
| Array | `*` | `*2\r\n...` | Lists of values |

### The CRLF Terminator

Every RESP message ends with `\r\n` (Carriage Return + Line Feed):
- `\r` = ASCII 13 (0x0D)
- `\n` = ASCII 10 (0x0A)

This is the same line ending used in HTTP, SMTP, and other protocols.

---

## 3. Type Prefixes

The first byte tells you the type:

```
+  Simple String (ASCII 43)
-  Error        (ASCII 45)
:  Integer      (ASCII 58)
$  Bulk String  (ASCII 36)
*  Array        (ASCII 42)
```

In Rust:

```rust
pub mod prefix {
    pub const SIMPLE_STRING: u8 = b'+';  // 43
    pub const ERROR: u8 = b'-';          // 45
    pub const INTEGER: u8 = b':';        // 58
    pub const BULK_STRING: u8 = b'$';    // 36
    pub const ARRAY: u8 = b'*';          // 42
}
```

---

## 4. Simple Strings

### Format

```
+<string>\r\n
```

### Examples

```
+OK\r\n        → "OK"
+PONG\r\n      → "PONG"
+hello\r\n     → "hello"
```

### Rules

- Cannot contain `\r` or `\n` (no newlines allowed)
- UTF-8 text only
- Used for short status responses

### In FlashKV

```rust
// Server response for successful SET
RespValue::SimpleString("OK".to_string())

// Serializes to:
b"+OK\r\n"
```

### When Used

- `SET` → `+OK`
- `PING` → `+PONG`
- `TYPE key` → `+string`

---

## 5. Errors

### Format

```
-<error message>\r\n
```

### Examples

```
-ERR unknown command 'foo'\r\n
-WRONGTYPE Operation against a key holding the wrong kind of value\r\n
```

### Convention

Error messages usually start with an error type:
- `ERR` - General error
- `WRONGTYPE` - Type mismatch
- `NOAUTH` - Authentication required

### In FlashKV

```rust
// Unknown command error
RespValue::Error("ERR unknown command 'foo'".to_string())

// Serializes to:
b"-ERR unknown command 'foo'\r\n"
```

---

## 6. Integers

### Format

```
:<number>\r\n
```

### Examples

```
:0\r\n        → 0
:1000\r\n     → 1000
:-1\r\n       → -1
```

### Range

64-bit signed integer: -2^63 to 2^63-1

### In FlashKV

```rust
// Response for INCR command
RespValue::Integer(42)

// Serializes to:
b":42\r\n"
```

### When Used

- `INCR`/`DECR` → new value
- `DEL` → count of deleted keys
- `EXISTS` → count of existing keys
- `TTL` → seconds remaining

---

## 7. Bulk Strings

### Format

```
$<length>\r\n<data>\r\n
```

### Examples

```
$5\r\nhello\r\n     → "hello" (5 bytes)
$0\r\n\r\n          → "" (empty string)
$-1\r\n             → null
```

### The Key Insight

The length prefix makes this binary-safe:

```
$11\r\nhello\r\nbye\r\n  → "hello\r\nbye" (contains \r\n!)
```

Because we know the length is 11, we read exactly 11 bytes, regardless of content.

### Null Bulk String

Length -1 means null (key doesn't exist):

```
$-1\r\n  → null
```

### In FlashKV

```rust
// Response with data
RespValue::BulkString(Bytes::from("hello"))
// Serializes to: $5\r\nhello\r\n

// Null response (key not found)
RespValue::Null
// Serializes to: $-1\r\n
```

### When Used

- `GET key` → the value (or null)
- Command arguments are bulk strings
- Any binary data

---

## 8. Arrays

### Format

```
*<count>\r\n<element1><element2>...
```

### Examples

Empty array:
```
*0\r\n
```

Array of two bulk strings:
```
*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n
```

Let's break that down:
```
*2\r\n           → Array with 2 elements
  $3\r\nfoo\r\n  → First element: "foo"
  $3\r\nbar\r\n  → Second element: "bar"
```

### Mixed Types

Arrays can contain any RESP type:

```
*3\r\n
:1\r\n           → Integer 1
$5\r\nhello\r\n  → Bulk string "hello"
+OK\r\n          → Simple string "OK"
```

### Nested Arrays

Arrays can contain arrays:

```
*2\r\n
*2\r\n:1\r\n:2\r\n    → [1, 2]
*2\r\n:3\r\n:4\r\n    → [3, 4]

Result: [[1, 2], [3, 4]]
```

### Null Array

```
*-1\r\n  → null (treated same as null bulk string)
```

### In FlashKV

```rust
// Response for KEYS command
RespValue::Array(vec![
    RespValue::BulkString(Bytes::from("user:1")),
    RespValue::BulkString(Bytes::from("user:2")),
])

// Serializes to:
// *2\r\n$6\r\nuser:1\r\n$6\r\nuser:2\r\n
```

---

## 9. Commands as Arrays

### The Rule

All Redis commands are sent as arrays of bulk strings.

### Example: PING

```
*1\r\n$4\r\nPING\r\n

Breakdown:
*1\r\n       → Array with 1 element
$4\r\nPING\r\n → "PING"
```

### Example: SET key value

```
*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$4\r\nAriz\r\n

Breakdown:
*3\r\n           → Array with 3 elements
$3\r\nSET\r\n    → "SET"
$4\r\nname\r\n   → "name"
$4\r\nAriz\r\n   → "Ariz"
```

### Example: GET key

```
*2\r\n$3\r\nGET\r\n$4\r\nname\r\n

Breakdown:
*2\r\n           → Array with 2 elements
$3\r\nGET\r\n    → "GET"
$4\r\nname\r\n   → "name"
```

### Example: MSET (multiple keys)

```
*5\r\n$4\r\nMSET\r\n$2\r\nk1\r\n$2\r\nv1\r\n$2\r\nk2\r\n$2\r\nv2\r\n

Breakdown:
*5\r\n           → Array with 5 elements
$4\r\nMSET\r\n   → "MSET"
$2\r\nk1\r\n     → "k1"
$2\r\nv1\r\n     → "v1"
$2\r\nk2\r\n     → "k2"
$2\r\nv2\r\n     → "v2"
```

### Inline Commands (Simple Format)

Redis also accepts a simpler format for testing:

```
PING\r\n
SET name Ariz\r\n
```

FlashKV doesn't implement this (but you could add it as an exercise!).

---

## 10. Parsing Strategy

### Step 1: Read the Type Prefix

```rust
match buf[0] {
    b'+' => parse_simple_string(buf),
    b'-' => parse_error(buf),
    b':' => parse_integer(buf),
    b'$' => parse_bulk_string(buf),
    b'*' => parse_array(buf),
    _ => Err(ParseError::UnknownPrefix(buf[0])),
}
```

### Step 2: Find CRLF

For simple types, find the line ending:

```rust
fn find_crlf(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}
```

### Step 3: Parse Content

For simple string `+hello\r\n`:
```rust
let end = find_crlf(&buf[1..])?;  // Find \r\n after +
let content = &buf[1..1 + end];    // "hello"
let consumed = 1 + end + 2;        // +hello\r\n = 8 bytes
```

### Step 4: Handle Bulk Strings

```rust
// $5\r\nhello\r\n
let length_end = find_crlf(&buf[1..])?;     // Position of first \r\n
let length: i64 = parse_int(&buf[1..1 + length_end])?;

if length == -1 {
    return Ok(RespValue::Null);
}

let data_start = 1 + length_end + 2;         // After $5\r\n
let data_end = data_start + length as usize;
let data = &buf[data_start..data_end];       // "hello"
let consumed = data_end + 2;                  // Including final \r\n
```

### Step 5: Handle Arrays (Recursive)

```rust
// *2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n
let count_end = find_crlf(&buf[1..])?;
let count: i64 = parse_int(&buf[1..1 + count_end])?;

let mut elements = Vec::new();
let mut pos = 1 + count_end + 2;  // After *2\r\n

for _ in 0..count {
    let (element, consumed) = parse_value(&buf[pos..])?;
    elements.push(element);
    pos += consumed;
}

return Ok((RespValue::Array(elements), pos));
```

### Handling Incomplete Data

The parser returns `Ok(None)` when there's not enough data:

```rust
fn parse(&mut self, buf: &[u8]) -> Result<Option<(RespValue, usize)>> {
    // If we can't find CRLF, we need more data
    let end = match find_crlf(&buf[1..]) {
        Some(pos) => pos,
        None => return Ok(None),  // Incomplete!
    };
    // ...
}
```

This is crucial for TCP streaming - we might receive partial messages!

---

## 11. Exercises

### Exercise 1: Manual Serialization

Write the RESP bytes for these commands BY HAND:

1. `PING`
2. `GET user:123`
3. `SET counter 42`
4. `MGET a b c`

### Exercise 2: Manual Parsing

Parse these RESP bytes into human-readable form:

1. `+PONG\r\n`
2. `:100\r\n`
3. `$-1\r\n`
4. `*2\r\n$5\r\nhello\r\n$5\r\nworld\r\n`
5. `*3\r\n:1\r\n:2\r\n*2\r\n:3\r\n:4\r\n`

### Exercise 3: Implement Inline Parser

Add support for inline commands:
- `PING` → same as `*1\r\n$4\r\nPING\r\n`
- `GET key` → same as `*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n`

### Exercise 4: Protocol Explorer

Write a program that:
1. Connects to FlashKV
2. Reads hex input from the user
3. Sends the bytes to the server
4. Shows the raw response in hex and parsed form

### Exercise 5: RESP Validator

Write a function that validates RESP data:
- Returns `Ok(())` if valid
- Returns `Err(description)` if invalid

Check for:
- Valid type prefix
- CRLF terminators
- Matching bulk string length
- Matching array count

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

1. `PING`:
   ```
   *1\r\n$4\r\nPING\r\n
   ```
   Bytes: `2a 31 0d 0a 24 34 0d 0a 50 49 4e 47 0d 0a`

2. `GET user:123`:
   ```
   *2\r\n$3\r\nGET\r\n$8\r\nuser:123\r\n
   ```

3. `SET counter 42`:
   ```
   *3\r\n$3\r\nSET\r\n$7\r\ncounter\r\n$2\r\n42\r\n
   ```

4. `MGET a b c`:
   ```
   *4\r\n$4\r\nMGET\r\n$1\r\na\r\n$1\r\nb\r\n$1\r\nc\r\n
   ```

### Exercise 2 Solution

1. `+PONG\r\n` → Simple String: "PONG"

2. `:100\r\n` → Integer: 100

3. `$-1\r\n` → Null (bulk string)

4. `*2\r\n$5\r\nhello\r\n$5\r\nworld\r\n` → Array: ["hello", "world"]

5. `*3\r\n:1\r\n:2\r\n*2\r\n:3\r\n:4\r\n` → Array: [1, 2, [3, 4]]

### Exercise 3 Solution

```rust
fn parse_inline(buf: &[u8]) -> Option<(RespValue, usize)> {
    // Find the line ending
    let end = find_crlf(buf)?;
    
    // Split by spaces
    let line = std::str::from_utf8(&buf[..end]).ok()?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    
    if parts.is_empty() {
        return None;
    }
    
    // Convert to array of bulk strings
    let elements: Vec<RespValue> = parts
        .into_iter()
        .map(|s| RespValue::BulkString(Bytes::from(s.to_string())))
        .collect();
    
    Some((RespValue::Array(elements), end + 2))
}
```

### Exercise 4 Solution

```rust
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{self, BufRead};

#[tokio::main]
async fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:6379").await.unwrap();
    let stdin = io::stdin();
    
    println!("RESP Explorer - Enter hex bytes (e.g., 2a31 0d0a 2434 0d0a 5049 4e47 0d0a)");
    println!("Type 'quit' to exit");
    
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        
        if line.trim() == "quit" {
            break;
        }
        
        // Parse hex input
        let bytes: Vec<u8> = line
            .split_whitespace()
            .flat_map(|s| {
                (0..s.len())
                    .step_by(2)
                    .filter_map(|i| u8::from_str_radix(&s[i..i+2], 16).ok())
            })
            .collect();
        
        if bytes.is_empty() {
            continue;
        }
        
        println!("Sending {} bytes", bytes.len());
        stream.write_all(&bytes).await.unwrap();
        
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        
        println!("Received {} bytes:", n);
        println!("  Hex: {:02x?}", &buf[..n]);
        println!("  Text: {}", String::from_utf8_lossy(&buf[..n]));
    }
}
```

### Exercise 5 Solution

```rust
fn validate_resp(buf: &[u8]) -> Result<(), String> {
    if buf.is_empty() {
        return Err("Empty input".to_string());
    }
    
    match buf[0] {
        b'+' | b'-' => validate_simple_line(buf),
        b':' => validate_integer(buf),
        b'$' => validate_bulk_string(buf),
        b'*' => validate_array(buf),
        other => Err(format!("Invalid type prefix: 0x{:02x}", other)),
    }
}

fn validate_simple_line(buf: &[u8]) -> Result<(), String> {
    if !ends_with_crlf(buf) {
        return Err("Missing CRLF terminator".to_string());
    }
    
    // Check no embedded newlines
    for i in 1..buf.len() - 2 {
        if buf[i] == b'\r' || buf[i] == b'\n' {
            return Err("Simple string contains newline".to_string());
        }
    }
    
    Ok(())
}

fn validate_integer(buf: &[u8]) -> Result<(), String> {
    let end = find_crlf(&buf[1..])
        .ok_or("Missing CRLF terminator")?;
    
    let num_str = std::str::from_utf8(&buf[1..1 + end])
        .map_err(|_| "Invalid UTF-8 in integer")?;
    
    num_str.parse::<i64>()
        .map_err(|_| format!("Invalid integer: {}", num_str))?;
    
    Ok(())
}

fn validate_bulk_string(buf: &[u8]) -> Result<(), String> {
    let len_end = find_crlf(&buf[1..])
        .ok_or("Missing length CRLF")?;
    
    let len_str = std::str::from_utf8(&buf[1..1 + len_end])
        .map_err(|_| "Invalid UTF-8 in length")?;
    
    let len: i64 = len_str.parse()
        .map_err(|_| format!("Invalid length: {}", len_str))?;
    
    if len == -1 {
        return Ok(());  // Null is valid
    }
    
    if len < 0 {
        return Err(format!("Invalid negative length: {}", len));
    }
    
    let data_start = 1 + len_end + 2;
    let expected_end = data_start + len as usize + 2;
    
    if buf.len() < expected_end {
        return Err("Truncated bulk string".to_string());
    }
    
    if buf[expected_end - 2] != b'\r' || buf[expected_end - 1] != b'\n' {
        return Err("Bulk string missing trailing CRLF".to_string());
    }
    
    Ok(())
}

fn validate_array(buf: &[u8]) -> Result<(), String> {
    let count_end = find_crlf(&buf[1..])
        .ok_or("Missing count CRLF")?;
    
    let count_str = std::str::from_utf8(&buf[1..1 + count_end])
        .map_err(|_| "Invalid UTF-8 in count")?;
    
    let count: i64 = count_str.parse()
        .map_err(|_| format!("Invalid count: {}", count_str))?;
    
    if count == -1 {
        return Ok(());  // Null array
    }
    
    if count < 0 {
        return Err(format!("Invalid negative count: {}", count));
    }
    
    // Would need to recursively validate each element...
    // Left as an exercise for the reader!
    
    Ok(())
}

fn ends_with_crlf(buf: &[u8]) -> bool {
    buf.len() >= 2 && buf[buf.len() - 2] == b'\r' && buf[buf.len() - 1] == b'\n'
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}
```

</details>

---

## Quick Reference Card

```
╔══════════════════════════════════════════════════════════════════╗
║                     RESP Quick Reference                          ║
╠══════════════════════════════════════════════════════════════════╣
║  Simple String:  +<string>\r\n                                    ║
║  Error:          -<message>\r\n                                   ║
║  Integer:        :<number>\r\n                                    ║
║  Bulk String:    $<length>\r\n<data>\r\n                          ║
║  Null:           $-1\r\n                                          ║
║  Array:          *<count>\r\n<element1><element2>...              ║
║  Null Array:     *-1\r\n                                          ║
╠══════════════════════════════════════════════════════════════════╣
║  All commands are arrays of bulk strings                          ║
║  Example: SET key value → *3\r\n$3\r\nSET\r\n$3\r\nkey\r\n...     ║
╚══════════════════════════════════════════════════════════════════╝
```

---

## Next Steps

Now that you understand RESP, let's see how FlashKV implements it:

**[05_PROTOCOL_TYPES.md](./05_PROTOCOL_TYPES.md)** - Deep dive into `src/protocol/types.rs`!