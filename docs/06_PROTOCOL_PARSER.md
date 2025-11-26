# Protocol Parser - `src/protocol/parser.rs` 🔍

This document provides a deep dive into the zero-copy RESP parser, one of the most technically interesting parts of FlashKV.

---

## Table of Contents

1. [File Overview](#1-file-overview)
2. [Design Philosophy](#2-design-philosophy)
3. [Error Types](#3-error-types)
4. [The RespParser Struct](#4-the-respparser-struct)
5. [Parsing Strategy](#5-parsing-strategy)
6. [Parsing Each Type](#6-parsing-each-type)
7. [Helper Functions](#7-helper-functions)
8. [Handling Incomplete Data](#8-handling-incomplete-data)
9. [Tests](#9-tests)
10. [Exercises](#10-exercises)

---

## 1. File Overview

### Purpose

This file implements a **zero-copy, incremental parser** for the RESP protocol. It converts raw bytes from the network into `RespValue` structures.

### Location

```
flashkv/src/protocol/parser.rs
```

### Key Features

| Feature | Description |
|---------|-------------|
| Zero-copy | Avoids copying data when possible using `Bytes` |
| Incremental | Can handle partial data, resuming when more arrives |
| Safe | Clear error handling, no panics |
| Recursive | Handles nested arrays naturally |

### Dependencies

```rust
use crate::protocol::types::{prefix, RespValue, CRLF};
use bytes::Bytes;
use std::num::ParseIntError;
use thiserror::Error;
```

---

## 2. Design Philosophy

### The Problem

Data arrives from the network in unpredictable chunks:

```
Chunk 1: "*3\r\n$3\r\nSET\r\n$4"
Chunk 2: "\r\nname\r\n$4\r\nAriz\r\n"
```

We need to:
1. Detect when we have a complete message
2. Parse it efficiently
3. Handle partial messages gracefully

### The Solution

Our parser returns one of three states:

```rust
pub fn parse(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>>
```

| Return Value | Meaning |
|--------------|---------|
| `Ok(Some((value, consumed)))` | Success! Parsed `value`, consumed `consumed` bytes |
| `Ok(None)` | Incomplete - need more data |
| `Err(e)` | Parse error - invalid protocol |

### Zero-Copy Parsing

Traditional parsing might copy data:

```rust
// Copies data!
let string = String::from_utf8(buffer[start..end].to_vec())?;
```

Our approach avoids copies:

```rust
// No copy - just reference counting
let data = Bytes::copy_from_slice(&buf[start..end]);
```

While `copy_from_slice` sounds like it copies (and it does once), the resulting `Bytes` can be cheaply cloned and sliced without further copying.

---

## 3. Error Types

### The ParseError Enum

```rust
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseError {
    #[error("empty input")]
    EmptyInput,

    #[error("unknown type prefix: {0:#04x}")]
    UnknownPrefix(u8),

    #[error("invalid integer: {0}")]
    InvalidInteger(String),

    #[error("invalid UTF-8: {0}")]
    InvalidUtf8(String),

    #[error("invalid bulk string length: {0}")]
    InvalidBulkLength(i64),

    #[error("invalid array length: {0}")]
    InvalidArrayLength(i64),

    #[error("protocol error: {0}")]
    ProtocolError(String),

    #[error("message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
}
```

### Understanding the Derive Macros

```rust
#[derive(Debug, Error, Clone, PartialEq)]
```

| Derive | From | Purpose |
|--------|------|---------|
| `Debug` | std | Enables `{:?}` formatting |
| `Error` | thiserror | Implements `std::error::Error` |
| `Clone` | std | Allows copying errors |
| `PartialEq` | std | Enables `==` comparison in tests |

### The `#[error(...)]` Attribute

From the `thiserror` crate - automatically implements `Display`:

```rust
#[error("invalid integer: {0}")]
InvalidInteger(String),

// This generates:
impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidInteger(s) => write!(f, "invalid integer: {}", s),
            // ...
        }
    }
}
```

### Error Variants Explained

| Variant | When It Occurs |
|---------|---------------|
| `EmptyInput` | Buffer is empty |
| `UnknownPrefix` | First byte isn't `+`, `-`, `:`, `$`, or `*` |
| `InvalidInteger` | Can't parse number (e.g., "abc" for length) |
| `InvalidUtf8` | Simple string contains invalid UTF-8 |
| `InvalidBulkLength` | Negative length (except -1 for null) |
| `InvalidArrayLength` | Negative count (except -1 for null) |
| `ProtocolError` | Missing CRLF, corrupted format |
| `MessageTooLarge` | Bulk string exceeds 512MB limit |

### Constants

```rust
/// Maximum size for a single bulk string (512 MB, same as Redis)
pub const MAX_BULK_SIZE: usize = 512 * 1024 * 1024;

/// Maximum array nesting depth (prevent stack overflow)
pub const MAX_NESTING_DEPTH: usize = 32;
```

These prevent denial-of-service attacks:
- Huge bulk strings could exhaust memory
- Deeply nested arrays could cause stack overflow

---

## 4. The RespParser Struct

### Definition

```rust
#[derive(Debug, Default)]
pub struct RespParser {
    /// Current nesting depth (for array parsing)
    depth: usize,
}
```

### Why a Struct?

You might wonder: why not just a function?

The struct allows us to:
1. Track nesting depth across recursive calls
2. Add state for future features (statistics, configuration)
3. Provide a clear API

### Creating a Parser

```rust
impl RespParser {
    pub fn new() -> Self {
        Self { depth: 0 }
    }
}
```

The `Default` derive also works:

```rust
let parser = RespParser::default();
```

### The Main Parse Method

```rust
pub fn parse(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    self.depth = 0;  // Reset depth for each top-level parse
    self.parse_value(buf)
}
```

This is the public API:
- Takes a byte slice
- Returns parsed value + bytes consumed, or None, or Error
- Resets depth before parsing

---

## 5. Parsing Strategy

### The Internal Method

```rust
fn parse_value(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    if buf.is_empty() {
        return Ok(None);  // Need more data
    }

    // Check nesting depth
    if self.depth > MAX_NESTING_DEPTH {
        return Err(ParseError::ProtocolError(format!(
            "maximum nesting depth exceeded: {}",
            MAX_NESTING_DEPTH
        )));
    }

    match buf[0] {
        prefix::SIMPLE_STRING => self.parse_simple_string(buf),
        prefix::ERROR => self.parse_error(buf),
        prefix::INTEGER => self.parse_integer(buf),
        prefix::BULK_STRING => self.parse_bulk_string(buf),
        prefix::ARRAY => self.parse_array(buf),
        other => Err(ParseError::UnknownPrefix(other)),
    }
}
```

### The Flow

```
                    buf[0]
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
       '+'           '$'           '*'
        │             │             │
        ▼             ▼             ▼
  SimpleString   BulkString      Array
                                   │
                           ┌───────┴───────┐
                           ▼               ▼
                      parse_value()   parse_value()
                       (recursive)     (recursive)
```

---

## 6. Parsing Each Type

### Simple Strings and Errors

These have the same format, just different prefix:

```rust
fn parse_simple_string(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    debug_assert!(buf[0] == prefix::SIMPLE_STRING);

    match find_crlf(&buf[1..]) {
        Some(pos) => {
            let content = &buf[1..1 + pos];
            let s = std::str::from_utf8(content)
                .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

            let consumed = 1 + pos + 2;  // prefix + content + CRLF
            Ok(Some((RespValue::SimpleString(s.to_string()), consumed)))
        }
        None => Ok(None),  // Incomplete
    }
}
```

**Step by Step**:

1. `debug_assert!` - Verify we're called correctly (removed in release builds)
2. `find_crlf(&buf[1..])` - Find `\r\n` after the prefix
3. If not found, return `Ok(None)` - incomplete data
4. Extract content between prefix and CRLF
5. Convert to UTF-8 string
6. Calculate total consumed bytes
7. Return the value and consumed count

**Example**:
```
buf = "+OK\r\n"
      ↑    ↑
      1    4 (position 3 in buf[1..])

pos = 2 (relative to buf[1..], so "OK" has length 2)
content = &buf[1..3] = "OK"
consumed = 1 + 2 + 2 = 5
```

### Integers

```rust
fn parse_integer(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    debug_assert!(buf[0] == prefix::INTEGER);

    match find_crlf(&buf[1..]) {
        Some(pos) => {
            let content = &buf[1..1 + pos];
            let s = std::str::from_utf8(content)
                .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

            let n: i64 = s
                .parse()
                .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;
            
            let consumed = 1 + pos + 2;
            Ok(Some((RespValue::Integer(n), consumed)))
        }
        None => Ok(None),
    }
}
```

Similar to simple strings, but parses as integer.

**Example**:
```
buf = ":1000\r\n"
content = "1000"
n = 1000i64
consumed = 7
```

### Bulk Strings (The Complex One)

```rust
fn parse_bulk_string(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    debug_assert!(buf[0] == prefix::BULK_STRING);

    // Step 1: Find the length line
    let length_end = match find_crlf(&buf[1..]) {
        Some(pos) => pos,
        None => return Ok(None),
    };

    // Step 2: Parse the length
    let length_str = std::str::from_utf8(&buf[1..1 + length_end])
        .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

    let length: i64 = length_str
        .parse()
        .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;

    // Step 3: Handle null bulk string
    if length == -1 {
        let consumed = 1 + length_end + 2;  // $-1\r\n
        return Ok(Some((RespValue::Null, consumed)));
    }

    // Step 4: Validate length
    if length < 0 {
        return Err(ParseError::InvalidBulkLength(length));
    }

    let length = length as usize;

    // Step 5: Check size limit
    if length > MAX_BULK_SIZE {
        return Err(ParseError::MessageTooLarge {
            size: length,
            max: MAX_BULK_SIZE,
        });
    }

    // Step 6: Calculate positions
    let data_start = 1 + length_end + 2;  // After "$<len>\r\n"
    let total_needed = data_start + length + 2;  // Plus data + trailing CRLF

    // Step 7: Check if we have enough data
    if buf.len() < total_needed {
        return Ok(None);  // Incomplete
    }

    // Step 8: Verify trailing CRLF
    if &buf[data_start + length..data_start + length + 2] != CRLF {
        return Err(ParseError::ProtocolError(
            "bulk string missing trailing CRLF".to_string(),
        ));
    }

    // Step 9: Extract the data
    let data = Bytes::copy_from_slice(&buf[data_start..data_start + length]);

    Ok(Some((RespValue::BulkString(data), total_needed)))
}
```

**Visual Example**:
```
buf = "$5\r\nhello\r\n"
       ↑ ↑  ↑     ↑
       0 2  4     11

length_end = 1 (position of \r in buf[1..])
length = 5
data_start = 1 + 1 + 2 = 4
total_needed = 4 + 5 + 2 = 11
data = buf[4..9] = "hello"
```

### Arrays (Recursive!)

```rust
fn parse_array(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    debug_assert!(buf[0] == prefix::ARRAY);

    // Step 1: Find the count line
    let count_end = match find_crlf(&buf[1..]) {
        Some(pos) => pos,
        None => return Ok(None),
    };

    // Step 2: Parse the count
    let count_str = std::str::from_utf8(&buf[1..1 + count_end])
        .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

    let count: i64 = count_str
        .parse()
        .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;

    // Step 3: Handle null array
    if count == -1 {
        let consumed = 1 + count_end + 2;
        return Ok(Some((RespValue::Null, consumed)));
    }

    // Step 4: Validate count
    if count < 0 {
        return Err(ParseError::InvalidArrayLength(count));
    }

    let count = count as usize;

    // Step 5: Parse each element
    let mut elements = Vec::with_capacity(count);
    let mut consumed = 1 + count_end + 2;  // After "*<count>\r\n"

    self.depth += 1;  // Track nesting

    for _ in 0..count {
        if consumed >= buf.len() {
            return Ok(None);  // Incomplete
        }

        match self.parse_value(&buf[consumed..])? {
            Some((value, element_consumed)) => {
                elements.push(value);
                consumed += element_consumed;
            }
            None => return Ok(None),  // Incomplete
        }
    }

    self.depth -= 1;

    Ok(Some((RespValue::Array(elements), consumed)))
}
```

**Key Insight**: The array parser calls `parse_value()` recursively for each element!

**Visual Example**:
```
buf = "*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"
       ↑  ↑ ↑         ↑
       0  3 4         14

After header: consumed = 4
Parse element 1 ($3\r\nfoo\r\n): consumed += 9 → 13
Parse element 2 ($3\r\nbar\r\n): consumed += 9 → 22
```

---

## 7. Helper Functions

### Finding CRLF

```rust
#[inline]
fn find_crlf(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}
```

**Breaking It Down**:

- `#[inline]` - Hint to compiler: inline this function for speed
- `saturating_sub(1)` - Prevents underflow (0 - 1 would panic with regular sub)
- Returns position of `\r`, not `\n`

**Why `saturating_sub`?**

```rust
buf.len().saturating_sub(1)

// If buf.len() is 0: 0.saturating_sub(1) = 0 (not panic!)
// If buf.len() is 1: 1.saturating_sub(1) = 0
// If buf.len() is 5: 5.saturating_sub(1) = 4
```

We need at least 2 bytes to find CRLF, so we loop up to `len - 1`.

### Convenience Function

```rust
pub fn parse_message(buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    RespParser::new().parse(buf)
}
```

For one-off parsing without creating a parser instance.

---

## 8. Handling Incomplete Data

### The Pattern

Throughout the parser, you'll see this pattern:

```rust
match find_crlf(&buf[1..]) {
    Some(pos) => {
        // We have enough data - continue parsing
    }
    None => Ok(None),  // Not enough data yet
}
```

And:

```rust
if buf.len() < total_needed {
    return Ok(None);
}
```

### How the Connection Handler Uses This

```rust
// In connection/handler.rs
loop {
    // Try to parse
    match self.parser.parse(&self.buffer) {
        Ok(Some((value, consumed))) => {
            // Got a complete message!
            self.buffer.advance(consumed);
            return Ok(Some(value));
        }
        Ok(None) => {
            // Need more data - read from socket
            self.read_more_data().await?;
        }
        Err(e) => {
            return Err(e);
        }
    }
}
```

### Why This Design?

**Attempt 1: Blocking**
```rust
// BAD - blocks until complete message
fn parse_blocking(socket: &mut TcpStream) -> RespValue {
    let mut buf = Vec::new();
    loop {
        socket.read_to_end(&mut buf);  // Blocks forever!
    }
}
```

**Attempt 2: Exceptions**
```rust
// BAD - uses exceptions for control flow
fn parse(buf: &[u8]) -> RespValue {
    if !enough_data(buf) {
        throw NeedMoreDataException;  // Rust doesn't have exceptions!
    }
}
```

**Our Design: Option**
```rust
// GOOD - returns None when incomplete
fn parse(buf: &[u8]) -> Result<Option<(RespValue, usize)>> {
    if !enough_data(buf) {
        return Ok(None);  // Clean, composable
    }
}
```

---

## 9. Tests

### Test Organization

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_string() {
        let input = b"+OK\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::SimpleString("OK".to_string()));
        assert_eq!(result.1, 5);
    }
    // ... many more tests
}
```

### What's Tested

| Test | Verifies |
|------|----------|
| `test_parse_simple_string` | Basic simple string |
| `test_parse_simple_string_incomplete` | Returns None for partial data |
| `test_parse_error` | Error messages |
| `test_parse_integer` | Positive integers |
| `test_parse_negative_integer` | Negative integers |
| `test_parse_bulk_string` | Normal bulk strings |
| `test_parse_null_bulk_string` | `$-1\r\n` → Null |
| `test_parse_empty_bulk_string` | `$0\r\n\r\n` → empty |
| `test_parse_bulk_string_incomplete` | Partial bulk string |
| `test_parse_array` | Array of bulk strings |
| `test_parse_null_array` | `*-1\r\n` → Null |
| `test_parse_empty_array` | `*0\r\n` → empty array |
| `test_parse_nested_array` | Arrays in arrays |
| `test_parse_mixed_array` | Different types in one array |
| `test_parse_unknown_prefix` | Error for invalid prefix |
| `test_parse_invalid_integer` | Error for non-numeric |
| `test_roundtrip` | Serialize → Parse → Same value |
| `test_parse_set_command` | Real SET command |
| `test_binary_safe_bulk_string` | Handles null bytes |

### The Roundtrip Test

```rust
#[test]
fn test_roundtrip() {
    let original = RespValue::Array(vec![
        RespValue::bulk_string(Bytes::from("SET")),
        RespValue::bulk_string(Bytes::from("key")),
        RespValue::bulk_string(Bytes::from("value")),
    ]);

    let serialized = original.serialize();
    let (parsed, _) = parse_message(&serialized).unwrap().unwrap();
    assert_eq!(original, parsed);
}
```

This is a powerful test: if we can serialize and parse back to the same value, both implementations are consistent!

---

## 10. Exercises

### Exercise 1: Add Inline Command Support

Redis also accepts simple text commands like `PING\r\n` instead of `*1\r\n$4\r\nPING\r\n`.

Modify the parser to detect and handle inline commands:

```rust
fn parse_inline(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    // If first byte is not a RESP prefix, try parsing as inline
    // Split by spaces, convert to array of bulk strings
}
```

### Exercise 2: Track Statistics

Add statistics to the parser:

```rust
pub struct RespParser {
    depth: usize,
    
    // Add these:
    pub messages_parsed: u64,
    pub bytes_parsed: u64,
    pub arrays_parsed: u64,
    pub max_depth_seen: usize,
}
```

Update the parsing methods to track these.

### Exercise 3: Streaming Parser

Create a `StreamingParser` that wraps a parser with its own buffer:

```rust
pub struct StreamingParser {
    parser: RespParser,
    buffer: BytesMut,
}

impl StreamingParser {
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }
    
    pub fn try_parse(&mut self) -> ParseResult<Option<RespValue>> {
        // Parse from buffer, advance if successful
    }
}
```

### Exercise 4: Better Error Messages

Improve error messages to include position:

```rust
#[error("unknown type prefix: {prefix:#04x} at position {position}")]
UnknownPrefix { prefix: u8, position: usize },
```

Modify parsing to track position and include it in errors.

### Exercise 5: Fuzzing

Write a fuzz test that generates random bytes and ensures the parser never panics:

```rust
// Using cargo-fuzz
fuzz_target!(|data: &[u8]| {
    let _ = parse_message(data);
    // Should never panic, just return Ok or Err
});
```

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
fn is_inline_command(buf: &[u8]) -> bool {
    !buf.is_empty() && !matches!(buf[0], b'+' | b'-' | b':' | b'$' | b'*')
}

fn parse_inline(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    let crlf_pos = match find_crlf(buf) {
        Some(pos) => pos,
        None => return Ok(None),
    };
    
    let line = std::str::from_utf8(&buf[..crlf_pos])
        .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;
    
    let parts: Vec<&str> = line.split_whitespace().collect();
    
    if parts.is_empty() {
        return Err(ParseError::ProtocolError("empty inline command".to_string()));
    }
    
    let elements: Vec<RespValue> = parts
        .into_iter()
        .map(|s| RespValue::BulkString(Bytes::from(s.to_string())))
        .collect();
    
    Ok(Some((RespValue::Array(elements), crlf_pos + 2)))
}

// In parse_value:
fn parse_value(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    if buf.is_empty() {
        return Ok(None);
    }
    
    match buf[0] {
        prefix::SIMPLE_STRING => self.parse_simple_string(buf),
        prefix::ERROR => self.parse_error(buf),
        prefix::INTEGER => self.parse_integer(buf),
        prefix::BULK_STRING => self.parse_bulk_string(buf),
        prefix::ARRAY => self.parse_array(buf),
        _ => self.parse_inline(buf),  // Try inline for unknown prefix
    }
}
```

### Exercise 2 Solution

```rust
#[derive(Debug, Default)]
pub struct RespParser {
    depth: usize,
    pub messages_parsed: u64,
    pub bytes_parsed: u64,
    pub arrays_parsed: u64,
    pub max_depth_seen: usize,
}

impl RespParser {
    pub fn parse(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        self.depth = 0;
        let result = self.parse_value(buf)?;
        
        if let Some((_, consumed)) = &result {
            self.messages_parsed += 1;
            self.bytes_parsed += *consumed as u64;
        }
        
        Ok(result)
    }
    
    fn parse_array(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        self.depth += 1;
        if self.depth > self.max_depth_seen {
            self.max_depth_seen = self.depth;
        }
        
        // ... rest of parsing ...
        
        self.arrays_parsed += 1;
        self.depth -= 1;
        
        // ... return result ...
    }
}
```

### Exercise 3 Solution

```rust
use bytes::BytesMut;

pub struct StreamingParser {
    parser: RespParser,
    buffer: BytesMut,
}

impl StreamingParser {
    pub fn new() -> Self {
        Self {
            parser: RespParser::new(),
            buffer: BytesMut::with_capacity(4096),
        }
    }
    
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }
    
    pub fn try_parse(&mut self) -> ParseResult<Option<RespValue>> {
        match self.parser.parse(&self.buffer)? {
            Some((value, consumed)) => {
                let _ = self.buffer.split_to(consumed);
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }
    
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }
}
```

### Exercise 4 Solution

```rust
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseError {
    #[error("empty input")]
    EmptyInput,

    #[error("unknown type prefix: {prefix:#04x} at position {position}")]
    UnknownPrefix { prefix: u8, position: usize },

    #[error("invalid integer at position {position}: {message}")]
    InvalidInteger { message: String, position: usize },
    
    // ... etc
}

// Track position through parsing:
fn parse_value(&mut self, buf: &[u8], pos: usize) -> ParseResult<Option<(RespValue, usize)>> {
    if buf.is_empty() {
        return Ok(None);
    }

    match buf[0] {
        prefix::SIMPLE_STRING => self.parse_simple_string(buf, pos),
        // ...
        other => Err(ParseError::UnknownPrefix { prefix: other, position: pos }),
    }
}
```

### Exercise 5 Solution

```rust
// In fuzz/fuzz_targets/parse.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use flashkv::protocol::parse_message;

fuzz_target!(|data: &[u8]| {
    // Should never panic - only Ok(...) or Err(...)
    let _ = parse_message(data);
});
```

Run with:
```bash
cargo install cargo-fuzz
cargo +nightly fuzz run parse
```

</details>

---

## Key Takeaways

1. **Incremental parsing** - Return `None` when incomplete, don't block
2. **Zero-copy where possible** - Use `Bytes` for efficient data handling
3. **Defensive limits** - Prevent DoS with size/depth limits
4. **Recursive descent** - Arrays naturally call back into `parse_value`
5. **Clear error types** - Each failure mode has its own variant

---

## Next Steps

Now that you understand the protocol layer, let's dive into the storage engine:

**[07_CONCURRENCY.md](./07_CONCURRENCY.md)** - Learn about thread safety and concurrent data structures!