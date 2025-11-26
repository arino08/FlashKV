# Rust Fundamentals Used in FlashKV 🦀

This document covers all the Rust concepts used throughout FlashKV. Even if you know Rust basics, this will help you understand *why* we use specific patterns in this project.

---

## Table of Contents

1. [Ownership and Borrowing](#1-ownership-and-borrowing)
2. [Smart Pointers](#2-smart-pointers)
3. [Enums and Pattern Matching](#3-enums-and-pattern-matching)
4. [Error Handling](#4-error-handling)
5. [Traits](#5-traits)
6. [Generics](#6-generics)
7. [Lifetimes](#7-lifetimes)
8. [Modules and Visibility](#8-modules-and-visibility)
9. [Macros](#9-macros)
10. [Exercises](#10-exercises)

---

## 1. Ownership and Borrowing

### The Problem

In C/C++, you manage memory manually:
```c
char* str = malloc(100);  // Allocate
// ... use str ...
free(str);                 // Must remember to free!
str = NULL;                // Must remember to null!
```

Bugs happen when you:
- Forget to free (memory leak)
- Free twice (double free)
- Use after free (use-after-free)

### Rust's Solution: Ownership

Every value in Rust has exactly ONE owner. When the owner goes out of scope, the value is dropped (freed).

```rust
fn main() {
    let s = String::from("hello");  // s owns the String
    
    // s goes out of scope here, String is automatically freed
}
```

### Moving Ownership

When you assign a value to another variable, ownership MOVES:

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1;  // Ownership moves from s1 to s2
    
    // println!("{}", s1);  // ERROR! s1 no longer owns anything
    println!("{}", s2);     // OK, s2 owns the String
}
```

### How FlashKV Uses This

In `storage/engine.rs`, when we store a key-value pair:

```rust
pub fn set(&self, key: Bytes, value: Bytes) -> bool {
    // key and value ownership is MOVED into this function
    // We then move them into the HashMap
    data.insert(key, Entry::new(value));
    // The HashMap now owns key and value
}
```

### Borrowing: References

Sometimes you want to USE a value without TAKING ownership. That's borrowing:

```rust
fn calculate_length(s: &String) -> usize {  // s is a REFERENCE
    s.len()
}  // s goes out of scope, but since it's just a reference, nothing is dropped

fn main() {
    let s = String::from("hello");
    let len = calculate_length(&s);  // Borrow s
    println!("{} has length {}", s, len);  // s is still valid!
}
```

### Mutable References

To modify borrowed data, use `&mut`:

```rust
fn add_world(s: &mut String) {
    s.push_str(" world");
}

fn main() {
    let mut s = String::from("hello");
    add_world(&mut s);
    println!("{}", s);  // "hello world"
}
```

### The Rules

1. You can have EITHER:
   - One mutable reference (`&mut T`), OR
   - Any number of immutable references (`&T`)
2. References must always be valid (no dangling pointers)

### How FlashKV Uses This

In the parser, we borrow the buffer to avoid copying:

```rust
pub fn parse(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    // buf is borrowed, we don't own it
    // This means we can parse without copying the data
}
```

---

## 2. Smart Pointers

### What Are Smart Pointers?

Smart pointers are data structures that act like pointers but have additional metadata and capabilities.

### `Box<T>` - Heap Allocation

`Box` puts data on the heap instead of the stack:

```rust
fn main() {
    let b = Box::new(5);  // 5 is stored on the heap
    println!("{}", b);    // Works like a regular reference
}
```

### `Rc<T>` - Reference Counting (Single-Threaded)

`Rc` allows multiple owners of the same data:

```rust
use std::rc::Rc;

fn main() {
    let data = Rc::new(String::from("hello"));
    let data2 = Rc::clone(&data);  // Now 2 owners
    let data3 = Rc::clone(&data);  // Now 3 owners
    
    println!("Count: {}", Rc::strong_count(&data));  // 3
}
// All three go out of scope, count drops to 0, data is freed
```

### `Arc<T>` - Atomic Reference Counting (Multi-Threaded)

`Arc` is like `Rc` but safe to share between threads:

```rust
use std::sync::Arc;
use std::thread;

fn main() {
    let data = Arc::new(vec![1, 2, 3]);
    
    let data_clone = Arc::clone(&data);
    let handle = thread::spawn(move || {
        println!("{:?}", data_clone);
    });
    
    println!("{:?}", data);
    handle.join().unwrap();
}
```

### How FlashKV Uses `Arc`

The storage engine is shared across ALL client connections:

```rust
// In main.rs
let storage = Arc::new(StorageEngine::new());

// For each connection...
let storage_clone = Arc::clone(&storage);
tokio::spawn(async move {
    // This task has its own reference to the same storage
    handle_connection(stream, addr, storage_clone).await;
});
```

**Why Arc?**
- Multiple tasks need access to the same storage
- `Arc` lets them share it safely
- When all connections close, the storage is automatically freed

---

## 3. Enums and Pattern Matching

### Basic Enums

```rust
enum Direction {
    North,
    South,
    East,
    West,
}

fn main() {
    let dir = Direction::North;
    
    match dir {
        Direction::North => println!("Going up!"),
        Direction::South => println!("Going down!"),
        Direction::East => println!("Going right!"),
        Direction::West => println!("Going left!"),
    }
}
```

### Enums with Data

Rust enums can hold data - this is POWERFUL:

```rust
enum Message {
    Quit,                       // No data
    Move { x: i32, y: i32 },    // Named fields (like a struct)
    Write(String),              // Single value
    ChangeColor(i32, i32, i32), // Multiple values (tuple)
}
```

### How FlashKV Uses Enums

The `RespValue` enum represents all possible Redis protocol values:

```rust
pub enum RespValue {
    SimpleString(String),      // +OK\r\n
    Error(String),             // -ERR message\r\n
    Integer(i64),              // :1000\r\n
    BulkString(Bytes),         // $5\r\nhello\r\n
    Null,                      // $-1\r\n
    Array(Vec<RespValue>),     // *2\r\n... (can contain any RespValue!)
}
```

### Pattern Matching

`match` is like a super-powered switch statement:

```rust
fn process_value(value: RespValue) {
    match value {
        RespValue::SimpleString(s) => {
            println!("Got simple string: {}", s);
        }
        RespValue::Integer(n) => {
            println!("Got integer: {}", n);
        }
        RespValue::Array(items) => {
            println!("Got array with {} items", items.len());
            for item in items {
                process_value(item);  // Recursion!
            }
        }
        RespValue::Null => {
            println!("Got null");
        }
        _ => {
            println!("Got something else");
        }
    }
}
```

### `if let` - Concise Pattern Matching

When you only care about one pattern:

```rust
// Instead of this:
match value {
    RespValue::BulkString(data) => {
        println!("Data: {:?}", data);
    }
    _ => {}
}

// Use this:
if let RespValue::BulkString(data) = value {
    println!("Data: {:?}", data);
}
```

### `matches!` Macro

Check if a value matches a pattern:

```rust
let value = RespValue::Null;

// Returns true or false
if matches!(value, RespValue::Null) {
    println!("It's null!");
}

// In FlashKV:
pub fn is_null(&self) -> bool {
    matches!(self, RespValue::Null)
}
```

---

## 4. Error Handling

### No Exceptions in Rust!

Rust doesn't have try/catch. Instead, it uses `Result` and `Option`.

### `Option<T>` - Maybe a Value

```rust
enum Option<T> {
    Some(T),  // There is a value
    None,     // There is no value
}
```

Example:

```rust
fn find_user(id: u32) -> Option<String> {
    if id == 1 {
        Some(String::from("Alice"))
    } else {
        None
    }
}

fn main() {
    match find_user(1) {
        Some(name) => println!("Found: {}", name),
        None => println!("User not found"),
    }
    
    // Or use if let:
    if let Some(name) = find_user(1) {
        println!("Found: {}", name);
    }
    
    // Or use unwrap_or:
    let name = find_user(999).unwrap_or(String::from("Unknown"));
}
```

### How FlashKV Uses `Option`

In `storage/engine.rs`:

```rust
pub fn get(&self, key: &Bytes) -> Option<Bytes> {
    // Returns Some(value) if key exists, None otherwise
    match data.get(key) {
        Some(entry) if !entry.is_expired() => Some(entry.value.clone()),
        _ => None,
    }
}
```

### `Result<T, E>` - Success or Error

```rust
enum Result<T, E> {
    Ok(T),   // Operation succeeded with value T
    Err(E),  // Operation failed with error E
}
```

Example:

```rust
fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err(String::from("Cannot divide by zero"))
    } else {
        Ok(a / b)
    }
}

fn main() {
    match divide(10, 2) {
        Ok(result) => println!("Result: {}", result),
        Err(e) => println!("Error: {}", e),
    }
}
```

### The `?` Operator - Error Propagation

The `?` operator returns early if there's an error:

```rust
fn read_file_length(path: &str) -> Result<usize, std::io::Error> {
    let content = std::fs::read_to_string(path)?;  // Returns Err if fails
    Ok(content.len())
}

// Equivalent to:
fn read_file_length_verbose(path: &str) -> Result<usize, std::io::Error> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    Ok(content.len())
}
```

### How FlashKV Uses `Result`

In the parser:

```rust
fn parse_integer(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    // ... find the line ...
    
    let n: i64 = s.parse()
        .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;
    //  ^^^^^^^^ Convert ParseIntError to our ParseError type
    //                                                                        ^ Return early if error
    
    Ok(Some((RespValue::Integer(n), consumed)))
}
```

### Custom Error Types with `thiserror`

FlashKV uses the `thiserror` crate for clean error definitions:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("empty input")]
    EmptyInput,
    
    #[error("unknown type prefix: {0:#04x}")]
    UnknownPrefix(u8),
    
    #[error("invalid integer: {0}")]
    InvalidInteger(String),
}
```

This automatically implements the `Display` and `Error` traits!

---

## 5. Traits

### What Are Traits?

Traits are like interfaces - they define behavior that types can implement.

```rust
trait Greet {
    fn greet(&self) -> String;
}

struct Person {
    name: String,
}

impl Greet for Person {
    fn greet(&self) -> String {
        format!("Hello, I'm {}!", self.name)
    }
}

struct Robot {
    id: u32,
}

impl Greet for Robot {
    fn greet(&self) -> String {
        format!("BEEP BOOP. UNIT {} ONLINE.", self.id)
    }
}
```

### Common Traits in FlashKV

#### `Debug` - For Printing with `{:?}`

```rust
#[derive(Debug)]
pub struct Entry {
    pub value: Bytes,
    pub expires_at: Option<Instant>,
}

// Now you can:
println!("{:?}", entry);
```

#### `Clone` - For Making Copies

```rust
#[derive(Clone)]
pub struct CommandHandler {
    storage: Arc<StorageEngine>,
}

// Now you can:
let handler2 = handler.clone();
```

#### `Default` - For Default Values

```rust
#[derive(Default)]
pub struct RespParser {
    depth: usize,  // Will be 0 by default
}

// Now you can:
let parser = RespParser::default();
```

#### `From` / `Into` - For Type Conversions

```rust
impl RespValue {
    pub fn bulk_string(data: impl Into<Bytes>) -> Self {
        RespValue::BulkString(data.into())
    }
}

// Now you can pass anything that converts to Bytes:
RespValue::bulk_string("hello");           // &str -> Bytes
RespValue::bulk_string(String::from("hi")); // String -> Bytes
RespValue::bulk_string(Bytes::from("yo"));  // Bytes -> Bytes
```

### Trait Bounds

Require types to implement certain traits:

```rust
fn print_all<T: Debug>(items: &[T]) {
    for item in items {
        println!("{:?}", item);
    }
}

// Or with where clause:
fn print_all<T>(items: &[T])
where
    T: Debug,
{
    for item in items {
        println!("{:?}", item);
    }
}
```

---

## 6. Generics

### Generic Functions

```rust
fn largest<T: PartialOrd>(list: &[T]) -> &T {
    let mut largest = &list[0];
    for item in list {
        if item > largest {
            largest = item;
        }
    }
    largest
}

fn main() {
    let numbers = vec![1, 5, 3, 9, 2];
    println!("Largest: {}", largest(&numbers));
    
    let chars = vec!['a', 'z', 'm'];
    println!("Largest: {}", largest(&chars));
}
```

### Generic Structs

```rust
struct Point<T> {
    x: T,
    y: T,
}

fn main() {
    let integer_point = Point { x: 5, y: 10 };
    let float_point = Point { x: 1.0, y: 4.5 };
}
```

### Generic Enums

You've already seen these:

```rust
enum Option<T> {
    Some(T),
    None,
}

enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

### How FlashKV Uses Generics

The `RespValue::bulk_string` method uses `impl Into<Bytes>`:

```rust
pub fn bulk_string(data: impl Into<Bytes>) -> Self {
    RespValue::BulkString(data.into())
}
```

This accepts ANY type that can be converted into `Bytes`.

---

## 7. Lifetimes

### The Problem

```rust
fn longest(x: &str, y: &str) -> &str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}
```

This won't compile! Rust doesn't know if the returned reference comes from `x` or `y`, so it doesn't know how long the return value lives.

### The Solution: Lifetime Annotations

```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}
```

This says: "The return value lives at least as long as BOTH `x` and `y`."

### Lifetime Elision

Rust can often figure out lifetimes automatically:

```rust
// You write:
fn first_word(s: &str) -> &str { ... }

// Rust understands:
fn first_word<'a>(s: &'a str) -> &'a str { ... }
```

### How FlashKV Handles Lifetimes

We mostly AVOID complex lifetimes by:

1. **Using owned types**: `String` instead of `&str`
2. **Using `Bytes`**: Which is reference-counted (like `Arc<[u8]>`)
3. **Cloning when needed**: Performance cost is usually negligible

Example - instead of:

```rust
fn get(&self, key: &Bytes) -> Option<&Bytes> {  // Complex lifetimes!
    // ...
}
```

We use:

```rust
fn get(&self, key: &Bytes) -> Option<Bytes> {  // Return owned clone
    Some(entry.value.clone())
}
```

---

## 8. Modules and Visibility

### Module Structure

```
src/
├── lib.rs          // Library root
├── main.rs         // Binary entry point
├── protocol/
│   ├── mod.rs      // Module declaration
│   ├── types.rs
│   └── parser.rs
└── storage/
    ├── mod.rs
    ├── engine.rs
    └── expiry.rs
```

### Declaring Modules

In `lib.rs`:

```rust
pub mod protocol;   // Load from protocol/mod.rs
pub mod storage;    // Load from storage/mod.rs
pub mod commands;
pub mod connection;
```

In `protocol/mod.rs`:

```rust
pub mod parser;     // Load from protocol/parser.rs
pub mod types;      // Load from protocol/types.rs

// Re-export for convenience
pub use parser::{RespParser, ParseError};
pub use types::RespValue;
```

### Visibility

- `pub` - Public, accessible from anywhere
- `pub(crate)` - Public within this crate only
- `pub(super)` - Public to parent module
- (nothing) - Private to current module

```rust
pub struct StorageEngine {
    // Private fields - can only be accessed within this module
    shards: Vec<Shard>,
    key_count: AtomicU64,
}

impl StorageEngine {
    // Public method - accessible from anywhere
    pub fn get(&self, key: &Bytes) -> Option<Bytes> { ... }
    
    // Private method - only accessible within this module
    fn shard_index(&self, key: &[u8]) -> usize { ... }
}
```

---

## 9. Macros

### Declarative Macros (`macro_rules!`)

```rust
// Define a macro
macro_rules! say_hello {
    () => {
        println!("Hello!");
    };
    ($name:expr) => {
        println!("Hello, {}!", $name);
    };
}

// Use it
say_hello!();           // Hello!
say_hello!("Alice");    // Hello, Alice!
```

### Common Macros in FlashKV

#### `vec![]` - Create a Vector

```rust
let v = vec![1, 2, 3];
```

#### `format!()` - Create a String

```rust
let s = format!("key:{}", id);
```

#### `println!()` / `eprintln!()` - Print Output

```rust
println!("Info: {}", message);
eprintln!("Error: {}", error);
```

#### `assert!()` / `assert_eq!()` - Testing

```rust
#[test]
fn test_something() {
    assert!(true);
    assert_eq!(2 + 2, 4);
}
```

### Derive Macros

Derive macros automatically implement traits:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    // ...
}
```

This auto-generates implementations for `Debug`, `Clone`, `PartialEq`, and `Eq`.

---

## 10. Exercises

### Exercise 1: Ownership Practice

What will this print? (Think before running!)

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1;
    // println!("{}", s1);  // Uncomment - what happens?
    println!("{}", s2);
}
```

**Fix it** by using a reference or clone.

### Exercise 2: Create an Enum

Create an enum `Command` with variants:
- `Get { key: String }`
- `Set { key: String, value: String }`
- `Delete { key: String }`
- `Ping`

Write a function `execute(cmd: Command) -> String` that returns appropriate responses.

### Exercise 3: Error Handling

Create a function `parse_port(s: &str) -> Result<u16, String>` that:
- Returns `Ok(port)` if `s` is a valid port number (1-65535)
- Returns `Err("Invalid number")` if parsing fails
- Returns `Err("Port out of range")` if number is out of range

### Exercise 4: Generics and Traits

Create a generic function `find_first<T, F>(items: &[T], predicate: F) -> Option<&T>`
where `F: Fn(&T) -> bool`.

It should return the first item where `predicate` returns `true`.

### Exercise 5: Implement a Simple Cache

Create a struct `SimpleCache` that:
- Stores `String` keys and `String` values
- Has `set(&mut self, key: String, value: String)`
- Has `get(&self, key: &str) -> Option<&String>`
- Has `delete(&mut self, key: &str) -> bool`

Use a `HashMap` internally.

---

## Solutions

<details>
<summary>Click to see solutions</summary>

### Exercise 1 Solution

```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1.clone();  // Clone to keep both
    println!("{}", s1);
    println!("{}", s2);
    
    // Or use references:
    let s1 = String::from("hello");
    let s2 = &s1;
    println!("{}", s1);
    println!("{}", s2);
}
```

### Exercise 2 Solution

```rust
enum Command {
    Get { key: String },
    Set { key: String, value: String },
    Delete { key: String },
    Ping,
}

fn execute(cmd: Command) -> String {
    match cmd {
        Command::Get { key } => format!("Getting {}", key),
        Command::Set { key, value } => format!("Setting {} = {}", key, value),
        Command::Delete { key } => format!("Deleting {}", key),
        Command::Ping => String::from("PONG"),
    }
}
```

### Exercise 3 Solution

```rust
fn parse_port(s: &str) -> Result<u16, String> {
    let n: u32 = s.parse().map_err(|_| String::from("Invalid number"))?;
    
    if n < 1 || n > 65535 {
        return Err(String::from("Port out of range"));
    }
    
    Ok(n as u16)
}
```

### Exercise 4 Solution

```rust
fn find_first<T, F>(items: &[T], predicate: F) -> Option<&T>
where
    F: Fn(&T) -> bool,
{
    for item in items {
        if predicate(item) {
            return Some(item);
        }
    }
    None
}

// Usage:
let numbers = vec![1, 2, 3, 4, 5];
let first_even = find_first(&numbers, |n| n % 2 == 0);
println!("{:?}", first_even);  // Some(2)
```

### Exercise 5 Solution

```rust
use std::collections::HashMap;

struct SimpleCache {
    data: HashMap<String, String>,
}

impl SimpleCache {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
    
    fn set(&mut self, key: String, value: String) {
        self.data.insert(key, value);
    }
    
    fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }
    
    fn delete(&mut self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }
}
```

</details>

---

## Next Steps

Now that you understand Rust fundamentals, move on to:

**[02_ASYNC_PROGRAMMING.md](./02_ASYNC_PROGRAMMING.md)** - Learn how FlashKV handles thousands of concurrent connections!