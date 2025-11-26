# Protocol Types - `src/protocol/types.rs` 📦

This document provides a deep dive into the `types.rs` file, which defines the core RESP data types used throughout FlashKV.

---

## Table of Contents

1. [File Overview](#1-file-overview)
2. [Constants and Prefixes](#2-constants-and-prefixes)
3. [The RespValue Enum](#3-the-respvalue-enum)
4. [Constructor Methods](#4-constructor-methods)
5. [Serialization](#5-serialization)
6. [Helper Methods](#6-helper-methods)
7. [Display Implementation](#7-display-implementation)
8. [Tests](#8-tests)
9. [Exercises](#9-exercises)

---

## 1. File Overview

### Purpose

This file defines:
- The `RespValue` enum representing all RESP data types
- Serialization logic to convert values to bytes
- Helper methods for creating and inspecting values

### Location

```
flashkv/src/protocol/types.rs
```

### Dependencies

```rust
use bytes::Bytes;
use std::fmt;
```

- `bytes::Bytes` - Efficient, reference-counted byte buffer
- `std::fmt` - For implementing `Display` trait

---

## 2. Constants and Prefixes

### The CRLF Terminator

```rust
/// The CRLF terminator used in RESP protocol
pub const CRLF: &[u8] = b"\r\n";
```

**What This Does**:
- Defines the line terminator as a byte slice
- `b"\r\n"` creates a byte string literal (type `&[u8; 2]`)
- Used when serializing RESP messages

**Why `&[u8]` instead of `&str`?**

We work with raw bytes, not text. While `\r\n` is valid UTF-8, treating it as bytes:
- Avoids UTF-8 validation overhead
- Is consistent with the rest of our byte handling
- Works seamlessly with binary data

### Type Prefixes

```rust
/// RESP protocol type prefixes
pub mod prefix {
    pub const SIMPLE_STRING: u8 = b'+';
    pub const ERROR: u8 = b'-';
    pub const INTEGER: u8 = b':';
    pub const BULK_STRING: u8 = b'$';
    pub const ARRAY: u8 = b'*';
}
```

**What This Does**:
- Defines a submodule containing the type prefix bytes
- Each constant is a single byte (`u8`)
- `b'+'` is a byte literal (character as ASCII value)

**Why a Module?**

Grouping related constants in a module:
- Provides namespacing: `prefix::SIMPLE_STRING` vs just `SIMPLE_STRING`
- Makes it clear these belong together
- Prevents name collisions

**Usage Example**:

```rust
use crate::protocol::types::prefix;

match buf[0] {
    prefix::SIMPLE_STRING => { /* ... */ }
    prefix::ERROR => { /* ... */ }
    _ => { /* ... */ }
}
```

---

## 3. The RespValue Enum

### Definition

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    /// Simple strings: +<string>\r\n
    SimpleString(String),

    /// Errors: -<error message>\r\n
    Error(String),

    /// 64-bit signed integers: :<integer>\r\n
    Integer(i64),

    /// Bulk strings: $<length>\r\n<data>\r\n
    BulkString(Bytes),

    /// Null value (null bulk string or null array)
    Null,

    /// Arrays: *<count>\r\n<element1><element2>...
    Array(Vec<RespValue>),
}
```

### Breaking It Down

#### The Derive Macros

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
```

| Trait | What It Does | Why We Need It |
|-------|--------------|----------------|
| `Debug` | Enables `{:?}` formatting | Debugging/logging |
| `Clone` | Enables `.clone()` | Copying values for responses |
| `PartialEq` | Enables `==` comparison | Testing equality |
| `Eq` | Marker for total equality | Required for some collections |

#### Each Variant Explained

**SimpleString(String)**
```rust
SimpleString(String)
```
- Holds a `String` (owned, heap-allocated text)
- Cannot contain `\r` or `\n`
- Used for short status messages like "OK" or "PONG"

**Error(String)**
```rust
Error(String)
```
- Also holds a `String`
- Represents error responses
- Same format as SimpleString, different meaning

**Integer(i64)**
```rust
Integer(i64)
```
- Holds a 64-bit signed integer
- Range: -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807
- Used for counts, TTLs, INCR results, etc.

**BulkString(Bytes)**
```rust
BulkString(Bytes)
```
- Holds `Bytes` (from the `bytes` crate)
- Binary-safe - can contain any bytes
- Used for all value data

**Why `Bytes` instead of `Vec<u8>` or `String`?**

`Bytes` is special:
- Reference-counted (cheap to clone)
- Immutable (thread-safe)
- Zero-copy slicing

```rust
let data = Bytes::from("hello world");
let hello = data.slice(0..5);   // No copy! Just adjusts pointers
let world = data.slice(6..11);  // Still no copy!
```

**Null**
```rust
Null
```
- Unit variant (no data)
- Represents `$-1\r\n` or `*-1\r\n`
- Used when a key doesn't exist

**Array(Vec<RespValue>)**
```rust
Array(Vec<RespValue>)
```
- Holds a vector of `RespValue` items
- **Recursive!** Arrays can contain arrays
- Used for commands and multi-value responses

---

## 4. Constructor Methods

### Why Constructors?

Instead of writing:
```rust
RespValue::SimpleString("OK".to_string())
```

We can write:
```rust
RespValue::simple_string("OK")
```

This is:
- More concise
- Handles type conversions automatically
- Provides a consistent API

### simple_string

```rust
pub fn simple_string(s: impl Into<String>) -> Self {
    RespValue::SimpleString(s.into())
}
```

**Breaking It Down**:

- `impl Into<String>` - Accepts anything that can become a `String`
- `s.into()` - Converts to `String`
- Returns `Self` (which is `RespValue`)

**What Can You Pass?**

```rust
RespValue::simple_string("OK");           // &str
RespValue::simple_string(String::from("OK"));  // String
RespValue::simple_string(format!("Hello {}", name));  // String
```

### error

```rust
pub fn error(s: impl Into<String>) -> Self {
    RespValue::Error(s.into())
}
```

Same pattern as `simple_string`.

### integer

```rust
pub fn integer(n: i64) -> Self {
    RespValue::Integer(n)
}
```

Simple wrapper - just takes the value directly.

### bulk_string

```rust
pub fn bulk_string(data: impl Into<Bytes>) -> Self {
    RespValue::BulkString(data.into())
}
```

**What Can You Pass?**

```rust
RespValue::bulk_string("hello");           // &str → Bytes
RespValue::bulk_string(String::from("hi")); // String → Bytes
RespValue::bulk_string(vec![1, 2, 3]);      // Vec<u8> → Bytes
RespValue::bulk_string(Bytes::from("yo"));  // Bytes → Bytes
```

### null

```rust
pub fn null() -> Self {
    RespValue::Null
}
```

### array

```rust
pub fn array(values: Vec<RespValue>) -> Self {
    RespValue::Array(values)
}
```

### Common Response Shortcuts

```rust
/// Common response for successful operations
pub fn ok() -> Self {
    RespValue::SimpleString("OK".to_string())
}

/// Common response for PONG
pub fn pong() -> Self {
    RespValue::SimpleString("PONG".to_string())
}
```

**Usage**:
```rust
// Instead of:
RespValue::SimpleString("OK".to_string())

// Just:
RespValue::ok()
```

---

## 5. Serialization

### The Main Method

```rust
pub fn serialize(&self) -> Vec<u8> {
    let mut buf = Vec::new();
    self.serialize_into(&mut buf);
    buf
}
```

**What It Does**:
1. Creates a new vector
2. Serializes into it
3. Returns the vector

### The Internal Method

```rust
pub fn serialize_into(&self, buf: &mut Vec<u8>) {
    match self {
        RespValue::SimpleString(s) => {
            buf.push(prefix::SIMPLE_STRING);
            buf.extend_from_slice(s.as_bytes());
            buf.extend_from_slice(CRLF);
        }
        // ... other variants
    }
}
```

**Why Two Methods?**

1. `serialize()` - Convenient, returns new vector
2. `serialize_into()` - Efficient, reuses existing buffer

For sending multiple responses:

```rust
// Less efficient - allocates for each response
let bytes1 = response1.serialize();
let bytes2 = response2.serialize();

// More efficient - reuses buffer
let mut buf = Vec::new();
response1.serialize_into(&mut buf);
response2.serialize_into(&mut buf);
```

### Serializing Each Type

**SimpleString**:
```rust
RespValue::SimpleString(s) => {
    buf.push(prefix::SIMPLE_STRING);      // +
    buf.extend_from_slice(s.as_bytes());  // OK
    buf.extend_from_slice(CRLF);          // \r\n
}
// Result: +OK\r\n
```

**Error**:
```rust
RespValue::Error(s) => {
    buf.push(prefix::ERROR);              // -
    buf.extend_from_slice(s.as_bytes());  // ERR message
    buf.extend_from_slice(CRLF);          // \r\n
}
// Result: -ERR message\r\n
```

**Integer**:
```rust
RespValue::Integer(n) => {
    buf.push(prefix::INTEGER);                      // :
    buf.extend_from_slice(n.to_string().as_bytes()); // 42
    buf.extend_from_slice(CRLF);                    // \r\n
}
// Result: :42\r\n
```

**BulkString**:
```rust
RespValue::BulkString(data) => {
    buf.push(prefix::BULK_STRING);                    // $
    buf.extend_from_slice(data.len().to_string().as_bytes()); // 5
    buf.extend_from_slice(CRLF);                      // \r\n
    buf.extend_from_slice(data);                      // hello
    buf.extend_from_slice(CRLF);                      // \r\n
}
// Result: $5\r\nhello\r\n
```

**Null**:
```rust
RespValue::Null => {
    buf.push(prefix::BULK_STRING);  // $
    buf.extend_from_slice(b"-1");   // -1
    buf.extend_from_slice(CRLF);    // \r\n
}
// Result: $-1\r\n
```

**Array (Recursive!)**:
```rust
RespValue::Array(values) => {
    buf.push(prefix::ARRAY);                           // *
    buf.extend_from_slice(values.len().to_string().as_bytes()); // 2
    buf.extend_from_slice(CRLF);                       // \r\n
    for value in values {
        value.serialize_into(buf);  // Recursive call!
    }
}
// For ["foo", "bar"]:
// Result: *2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n
```

---

## 6. Helper Methods

### Type Checking

```rust
/// Returns true if this value is null.
pub fn is_null(&self) -> bool {
    matches!(self, RespValue::Null)
}

/// Returns true if this value is an error.
pub fn is_error(&self) -> bool {
    matches!(self, RespValue::Error(_))
}
```

**The `matches!` Macro**:
```rust
// matches!(value, pattern) returns true if value matches pattern

matches!(self, RespValue::Null)
// Equivalent to:
match self {
    RespValue::Null => true,
    _ => false,
}
```

### Extractors

These methods try to extract the inner value:

```rust
/// Attempts to extract the inner string from SimpleString or BulkString.
pub fn as_str(&self) -> Option<&str> {
    match self {
        RespValue::SimpleString(s) => Some(s),
        RespValue::BulkString(b) => std::str::from_utf8(b).ok(),
        _ => None,
    }
}
```

**Note**: `BulkString` might not be valid UTF-8, so we use `from_utf8().ok()` which returns `None` on invalid UTF-8.

```rust
/// Attempts to extract the inner bytes from BulkString.
pub fn as_bytes(&self) -> Option<&[u8]> {
    match self {
        RespValue::BulkString(b) => Some(b),
        _ => None,
    }
}

/// Attempts to extract the inner integer.
pub fn as_integer(&self) -> Option<i64> {
    match self {
        RespValue::Integer(n) => Some(*n),
        _ => None,
    }
}

/// Attempts to extract the inner array.
pub fn as_array(&self) -> Option<&[RespValue]> {
    match self {
        RespValue::Array(arr) => Some(arr),
        _ => None,
    }
}
```

### Consuming Extractor

```rust
/// Consumes self and returns the inner array if this is an Array variant.
pub fn into_array(self) -> Option<Vec<RespValue>> {
    match self {
        RespValue::Array(arr) => Some(arr),
        _ => None,
    }
}
```

**Difference from `as_array()`**:
- `as_array(&self)` - Borrows, returns reference
- `into_array(self)` - Consumes, returns owned value

Use `into_array()` when you want to take ownership:

```rust
// With as_array - we borrow
if let Some(elements) = value.as_array() {
    // value is still valid
    // elements is &[RespValue]
}

// With into_array - we consume
if let Some(elements) = value.into_array() {
    // value is gone (moved)
    // elements is Vec<RespValue>
}
```

---

## 7. Display Implementation

```rust
impl fmt::Display for RespValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RespValue::SimpleString(s) => write!(f, "\"{}\"", s),
            RespValue::Error(s) => write!(f, "(error) {}", s),
            RespValue::Integer(n) => write!(f, "(integer) {}", n),
            RespValue::BulkString(data) => {
                if let Ok(s) = std::str::from_utf8(data) {
                    write!(f, "\"{}\"", s)
                } else {
                    write!(f, "(binary data, {} bytes)", data.len())
                }
            }
            RespValue::Null => write!(f, "(nil)"),
            RespValue::Array(values) => {
                if values.is_empty() {
                    write!(f, "(empty array)")
                } else {
                    writeln!(f)?;
                    for (i, v) in values.iter().enumerate() {
                        writeln!(f, "{}) {}", i + 1, v)?;
                    }
                    Ok(())
                }
            }
        }
    }
}
```

**What It Does**:
- Implements the `Display` trait for `{}`  formatting
- Formats values similar to `redis-cli` output

**Examples**:
```rust
println!("{}", RespValue::simple_string("OK"));
// Output: "OK"

println!("{}", RespValue::integer(42));
// Output: (integer) 42

println!("{}", RespValue::null());
// Output: (nil)

println!("{}", RespValue::array(vec![
    RespValue::integer(1),
    RespValue::integer(2),
]));
// Output:
// 1) (integer) 1
// 2) (integer) 2
```

---

## 8. Tests

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string_serialize() {
        let value = RespValue::simple_string("OK");
        assert_eq!(value.serialize(), b"+OK\r\n");
    }
    // ... more tests
}
```

**Key Points**:
- `#[cfg(test)]` - Only compiled during testing
- `use super::*` - Import everything from parent module
- `#[test]` - Marks a function as a test

### What The Tests Cover

| Test | What It Verifies |
|------|-----------------|
| `test_simple_string_serialize` | `+OK\r\n` format |
| `test_error_serialize` | `-ERR message\r\n` format |
| `test_integer_serialize` | `:1000\r\n` and negative numbers |
| `test_bulk_string_serialize` | `$5\r\nhello\r\n` format |
| `test_null_serialize` | `$-1\r\n` format |
| `test_array_serialize` | Array with bulk strings |
| `test_nested_array_serialize` | Arrays containing arrays |
| `test_ok_response` | `RespValue::ok()` helper |
| `test_pong_response` | `RespValue::pong()` helper |

### Running Tests

```bash
# Run all tests
cargo test

# Run only protocol tests
cargo test protocol::types

# Run with output visible
cargo test -- --nocapture
```

---

## 9. Exercises

### Exercise 1: Add a Helper

Add a `bulk_string_from_str` method that's more explicit:

```rust
pub fn bulk_string_from_str(s: &str) -> Self {
    // Your implementation
}
```

### Exercise 2: Add Type Checking

Implement `is_string()` that returns true for both `SimpleString` and `BulkString`:

```rust
pub fn is_string(&self) -> bool {
    // Your implementation
}
```

### Exercise 3: Add Array Helper

Implement a method to create an array from string slices:

```rust
pub fn array_of_strings(strings: &[&str]) -> Self {
    // Convert each string to a BulkString and wrap in Array
}
```

### Exercise 4: Size Estimation

Implement a method to estimate serialized size without actually serializing:

```rust
pub fn serialized_size(&self) -> usize {
    // Calculate how many bytes serialize() would produce
}
```

### Exercise 5: Recursive Depth

Implement a method to find the maximum nesting depth:

```rust
pub fn depth(&self) -> usize {
    // Return 0 for non-arrays, 1 for flat arrays, etc.
}
```

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
pub fn bulk_string_from_str(s: &str) -> Self {
    RespValue::BulkString(Bytes::from(s.to_string()))
}
```

### Exercise 2 Solution

```rust
pub fn is_string(&self) -> bool {
    matches!(self, RespValue::SimpleString(_) | RespValue::BulkString(_))
}
```

### Exercise 3 Solution

```rust
pub fn array_of_strings(strings: &[&str]) -> Self {
    RespValue::Array(
        strings
            .iter()
            .map(|s| RespValue::bulk_string(Bytes::from(s.to_string())))
            .collect()
    )
}
```

### Exercise 4 Solution

```rust
pub fn serialized_size(&self) -> usize {
    match self {
        RespValue::SimpleString(s) => 1 + s.len() + 2,  // +<string>\r\n
        RespValue::Error(s) => 1 + s.len() + 2,         // -<error>\r\n
        RespValue::Integer(n) => {
            let num_str = n.to_string();
            1 + num_str.len() + 2  // :<number>\r\n
        }
        RespValue::BulkString(data) => {
            let len_str = data.len().to_string();
            1 + len_str.len() + 2 + data.len() + 2  // $<len>\r\n<data>\r\n
        }
        RespValue::Null => 5,  // $-1\r\n
        RespValue::Array(values) => {
            let count_str = values.len().to_string();
            let header = 1 + count_str.len() + 2;  // *<count>\r\n
            let elements: usize = values.iter().map(|v| v.serialized_size()).sum();
            header + elements
        }
    }
}
```

### Exercise 5 Solution

```rust
pub fn depth(&self) -> usize {
    match self {
        RespValue::Array(values) => {
            1 + values.iter().map(|v| v.depth()).max().unwrap_or(0)
        }
        _ => 0,
    }
}
```

</details>

---

## Key Takeaways

1. **Enums with data** - Rust enums are powerful, each variant can hold different data
2. **`impl Into<T>`** - Makes APIs flexible and ergonomic
3. **`Bytes`** - Efficient byte handling with zero-copy operations
4. **Recursive types** - Arrays containing arrays work naturally
5. **Serialize vs serialize_into** - Provide both convenience and efficiency

---

## Next Steps

Now that you understand the types, let's see how they're parsed:

**[06_PROTOCOL_PARSER.md](./06_PROTOCOL_PARSER.md)** - Deep dive into `src/protocol/parser.rs`!