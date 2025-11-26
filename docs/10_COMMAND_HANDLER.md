# Command Handler - `src/commands/handler.rs` 🎮

This document explains the command handler, which processes all Redis commands and interacts with the storage engine.

---

## Table of Contents

1. [File Overview](#1-file-overview)
2. [The CommandHandler Struct](#2-the-commandhandler-struct)
3. [Command Dispatch](#3-command-dispatch)
4. [Helper Methods](#4-helper-methods)
5. [String Commands](#5-string-commands)
6. [List Commands](#6-list-commands)
7. [Key Commands](#7-key-commands)
8. [Server Commands](#8-server-commands)
9. [Adding New Commands](#9-adding-new-commands)
10. [Error Handling](#10-error-handling)
11. [Exercises](#11-exercises)

---

## 1. File Overview

### Purpose

The command handler is the bridge between the protocol layer and the storage layer:

```
┌─────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ RespValue   │ ──> │ CommandHandler  │ ──> │ StorageEngine   │
│ (parsed)    │     │ (dispatch)      │     │ (data)          │
└─────────────┘     └─────────────────┘     └─────────────────┘
                            │
                            ▼
                    ┌─────────────┐
                    │ RespValue   │
                    │ (response)  │
                    └─────────────┘
```

### Location

```
flashkv/src/commands/handler.rs
```

### Key Dependencies

```rust
use crate::protocol::RespValue;
use crate::storage::StorageEngine;
use bytes::Bytes;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
```

---

## 2. The CommandHandler Struct

### Definition

```rust
#[derive(Clone)]
pub struct CommandHandler {
    /// The storage engine
    storage: Arc<StorageEngine>,
    /// Server start time for INFO command
    start_time: std::time::Instant,
}
```

### Why Clone?

The handler is cloned for each connection, but they all share the same storage via `Arc`:

```rust
// One storage, many handlers
let storage = Arc::new(StorageEngine::new());

// Each connection gets its own handler
let handler1 = CommandHandler::new(Arc::clone(&storage));
let handler2 = CommandHandler::new(Arc::clone(&storage));
// Both point to the same storage!
```

### Constructor

```rust
impl CommandHandler {
    pub fn new(storage: Arc<StorageEngine>) -> Self {
        Self {
            storage,
            start_time: std::time::Instant::now(),
        }
    }
}
```

---

## 3. Command Dispatch

### The Main Entry Point

```rust
pub fn execute(&self, command: RespValue) -> RespValue {
    // Commands should be arrays
    let args = match command {
        RespValue::Array(args) => args,
        _ => {
            return RespValue::error("ERR invalid command format");
        }
    };

    if args.is_empty() {
        return RespValue::error("ERR empty command");
    }

    // Extract command name (first argument)
    let cmd_name = match &args[0] {
        RespValue::BulkString(s) => match std::str::from_utf8(s) {
            Ok(s) => s.to_uppercase(),
            Err(_) => return RespValue::error("ERR invalid command name"),
        },
        RespValue::SimpleString(s) => s.to_uppercase(),
        _ => return RespValue::error("ERR invalid command name"),
    };

    // Dispatch to appropriate handler
    self.dispatch(&cmd_name, &args[1..])
}
```

### The Dispatch Table

```rust
fn dispatch(&self, cmd: &str, args: &[RespValue]) -> RespValue {
    match cmd {
        // String commands
        "SET" => self.cmd_set(args),
        "GET" => self.cmd_get(args),
        "DEL" => self.cmd_del(args),
        "EXISTS" => self.cmd_exists(args),
        "APPEND" => self.cmd_append(args),
        "STRLEN" => self.cmd_strlen(args),
        "INCR" => self.cmd_incr(args),
        "INCRBY" => self.cmd_incrby(args),
        "DECR" => self.cmd_decr(args),
        "DECRBY" => self.cmd_decrby(args),
        "MSET" => self.cmd_mset(args),
        "MGET" => self.cmd_mget(args),
        "SETNX" => self.cmd_setnx(args),
        "SETEX" => self.cmd_setex(args),
        "PSETEX" => self.cmd_psetex(args),
        "GETSET" => self.cmd_getset(args),
        "GETDEL" => self.cmd_getdel(args),

        // List commands
        "LPUSH" => self.cmd_lpush(args),
        "RPUSH" => self.cmd_rpush(args),
        "LPOP" => self.cmd_lpop(args),
        "RPOP" => self.cmd_rpop(args),
        "LLEN" => self.cmd_llen(args),
        "LINDEX" => self.cmd_lindex(args),
        "LRANGE" => self.cmd_lrange(args),
        "LSET" => self.cmd_lset(args),
        "LREM" => self.cmd_lrem(args),

        // Key commands
        "EXPIRE" => self.cmd_expire(args),
        "PEXPIRE" => self.cmd_pexpire(args),
        "EXPIREAT" => self.cmd_expireat(args),
        "TTL" => self.cmd_ttl(args),
        "PTTL" => self.cmd_pttl(args),
        "PERSIST" => self.cmd_persist(args),
        "KEYS" => self.cmd_keys(args),
        "TYPE" => self.cmd_type(args),
        "RENAME" => self.cmd_rename(args),
        "RENAMENX" => self.cmd_renamenx(args),

        // Server commands
        "PING" => self.cmd_ping(args),
        "ECHO" => self.cmd_echo(args),
        "INFO" => self.cmd_info(args),
        "DBSIZE" => self.cmd_dbsize(args),
        "FLUSHDB" | "FLUSHALL" => self.cmd_flushdb(args),
        "COMMAND" => self.cmd_command(args),
        "CONFIG" => self.cmd_config(args),
        "TIME" => self.cmd_time(args),
        "QUIT" => RespValue::ok(),

        // Unknown command
        _ => RespValue::error(format!("ERR unknown command '{}'", cmd)),
    }
}
```

### Command Flow Example

```
Client sends: SET name Ariz

1. Bytes: *3\r\n$3\r\nSET\r\n$4\r\nname\r\n$4\r\nAriz\r\n

2. Parser creates:
   RespValue::Array([
     BulkString("SET"),
     BulkString("name"),
     BulkString("Ariz")
   ])

3. execute() extracts:
   cmd_name = "SET"
   args = [BulkString("name"), BulkString("Ariz")]

4. dispatch() calls cmd_set(args)

5. cmd_set():
   - Extracts key = Bytes("name")
   - Extracts value = Bytes("Ariz")
   - Calls storage.set(key, value)
   - Returns RespValue::ok()

6. Response serialized: +OK\r\n
```

---

## 4. Helper Methods

### Extracting Bytes

```rust
fn get_bytes(&self, value: &RespValue) -> Option<Bytes> {
    match value {
        RespValue::BulkString(b) => Some(b.clone()),
        RespValue::SimpleString(s) => Some(Bytes::from(s.clone())),
        _ => None,
    }
}
```

### Extracting Strings

```rust
fn get_string(&self, value: &RespValue) -> Option<String> {
    match value {
        RespValue::BulkString(b) => std::str::from_utf8(b).ok().map(|s| s.to_string()),
        RespValue::SimpleString(s) => Some(s.clone()),
        _ => None,
    }
}
```

### Extracting Integers

```rust
fn get_integer(&self, value: &RespValue) -> Option<i64> {
    match value {
        RespValue::Integer(n) => Some(*n),
        RespValue::BulkString(b) => std::str::from_utf8(b).ok().and_then(|s| s.parse().ok()),
        RespValue::SimpleString(s) => s.parse().ok(),
        _ => None,
    }
}
```

### Why These Helpers?

Redis accepts arguments in different formats:
- `SET key 42` - "42" as bulk string
- Protocol allows integers directly

These helpers normalize the input so command implementations don't worry about format.

---

## 5. String Commands

### SET Command

The most complex command due to its many options:

```rust
/// SET key value [EX seconds] [PX milliseconds] [NX|XX]
fn cmd_set(&self, args: &[RespValue]) -> RespValue {
    if args.len() < 2 {
        return RespValue::error("ERR wrong number of arguments for 'SET' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    let value = match self.get_bytes(&args[1]) {
        Some(v) => v,
        None => return RespValue::error("ERR invalid value"),
    };

    // Parse optional arguments
    let mut ttl: Option<Duration> = None;
    let mut nx = false;  // Only set if not exists
    let mut xx = false;  // Only set if exists

    let mut i = 2;
    while i < args.len() {
        let opt = match self.get_string(&args[i]) {
            Some(s) => s.to_uppercase(),
            None => return RespValue::error("ERR invalid option"),
        };

        match opt.as_str() {
            "EX" => {
                i += 1;
                if i >= args.len() {
                    return RespValue::error("ERR syntax error");
                }
                let secs = match self.get_integer(&args[i]) {
                    Some(s) if s > 0 => s as u64,
                    _ => return RespValue::error("ERR invalid expire time"),
                };
                ttl = Some(Duration::from_secs(secs));
            }
            "PX" => {
                i += 1;
                if i >= args.len() {
                    return RespValue::error("ERR syntax error");
                }
                let ms = match self.get_integer(&args[i]) {
                    Some(m) if m > 0 => m as u64,
                    _ => return RespValue::error("ERR invalid expire time"),
                };
                ttl = Some(Duration::from_millis(ms));
            }
            "NX" => nx = true,
            "XX" => xx = true,
            _ => return RespValue::error(format!("ERR unknown option '{}'", opt)),
        }
        i += 1;
    }

    // Handle NX/XX conditions
    let exists = self.storage.exists(&key);

    if nx && exists {
        return RespValue::null();
    }

    if xx && !exists {
        return RespValue::null();
    }

    // Perform the SET
    match ttl {
        Some(duration) => self.storage.set_with_ttl(key, value, duration),
        None => self.storage.set(key, value),
    };

    RespValue::ok()
}
```

### GET Command

Simple and direct:

```rust
fn cmd_get(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 1 {
        return RespValue::error("ERR wrong number of arguments for 'GET' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    match self.storage.get(&key) {
        Some(value) => RespValue::bulk_string(value),
        None => RespValue::null(),
    }
}
```

### INCR/INCRBY

```rust
fn cmd_incr(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 1 {
        return RespValue::error("ERR wrong number of arguments for 'INCR' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    match self.storage.incr(&key) {
        Ok(n) => RespValue::integer(n),
        Err(e) => RespValue::error(format!("ERR {}", e)),
    }
}

fn cmd_incrby(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 2 {
        return RespValue::error("ERR wrong number of arguments for 'INCRBY' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    let delta = match self.get_integer(&args[1]) {
        Some(d) => d,
        None => return RespValue::error("ERR value is not an integer"),
    };

    match self.storage.incr_by(&key, delta) {
        Ok(n) => RespValue::integer(n),
        Err(e) => RespValue::error(format!("ERR {}", e)),
    }
}
```

### MGET (Multiple GET)

Returns an array:

```rust
fn cmd_mget(&self, args: &[RespValue]) -> RespValue {
    if args.is_empty() {
        return RespValue::error("ERR wrong number of arguments for 'MGET' command");
    }

    let values: Vec<RespValue> = args
        .iter()
        .map(|arg| match self.get_bytes(arg) {
            Some(key) => match self.storage.get(&key) {
                Some(v) => RespValue::bulk_string(v),
                None => RespValue::null(),
            },
            None => RespValue::null(),
        })
        .collect();

    RespValue::array(values)
}
```

---

## 6. List Commands

FlashKV supports Redis-compatible list operations. Lists are stored separately from strings and use a `VecDeque` internally for O(1) push/pop operations on both ends.

### LPUSH

Pushes one or more values to the head (left) of a list. Creates the list if it doesn't exist.

```rust
/// LPUSH key value [value ...]
fn cmd_lpush(&self, args: &[RespValue]) -> RespValue {
    if args.len() < 2 {
        return RespValue::error("ERR wrong number of arguments for 'LPUSH' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    // Check if key exists as a string (type error)
    if self.storage.exists(&key) {
        return RespValue::error(
            "WRONGTYPE Operation against a key holding the wrong kind of value",
        );
    }

    let mut values = Vec::with_capacity(args.len() - 1);
    for arg in &args[1..] {
        match self.get_bytes(arg) {
            Some(v) => values.push(v),
            None => return RespValue::error("ERR invalid value"),
        }
    }

    let len = self.storage.lpush(key, values);
    RespValue::integer(len as i64)
}
```

**Example:**
```
LPUSH mylist "world"     -> (integer) 1
LPUSH mylist "hello"     -> (integer) 2
LRANGE mylist 0 -1       -> 1) "hello"  2) "world"
```

### RPUSH

Pushes one or more values to the tail (right) of a list.

```rust
/// RPUSH key value [value ...]
fn cmd_rpush(&self, args: &[RespValue]) -> RespValue {
    // Similar to LPUSH but pushes to the tail
    let len = self.storage.rpush(key, values);
    RespValue::integer(len as i64)
}
```

### LPOP / RPOP

Remove and return elements from the head or tail of a list.

```rust
/// LPOP key
fn cmd_lpop(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    match self.storage.lpop(&key) {
        Some(v) => RespValue::bulk_string(v),
        None => RespValue::null(),
    }
}

/// RPOP key
fn cmd_rpop(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    match self.storage.rpop(&key) {
        Some(v) => RespValue::bulk_string(v),
        None => RespValue::null(),
    }
}
```

### LLEN

Returns the length of a list.

```rust
/// LLEN key
fn cmd_llen(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    let len = self.storage.llen(&key);
    RespValue::integer(len as i64)
}
```

### LINDEX

Returns the element at the specified index. Negative indices count from the end (-1 is the last element).

```rust
/// LINDEX key index
fn cmd_lindex(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    match self.storage.lindex(&key, index) {
        Some(v) => RespValue::bulk_string(v),
        None => RespValue::null(),
    }
}
```

**Example:**
```
RPUSH mylist "a" "b" "c"
LINDEX mylist 0      -> "a"
LINDEX mylist -1     -> "c"
LINDEX mylist 100    -> (nil)
```

### LRANGE

Returns a range of elements from a list. Both start and stop are inclusive. Negative indices are supported.

```rust
/// LRANGE key start stop
fn cmd_lrange(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    let elements = self.storage.lrange(&key, start, stop);
    let values: Vec<RespValue> = elements.into_iter().map(RespValue::bulk_string).collect();
    RespValue::array(values)
}
```

**Example:**
```
RPUSH mylist "a" "b" "c" "d" "e"
LRANGE mylist 0 -1    -> 1) "a" 2) "b" 3) "c" 4) "d" 5) "e"
LRANGE mylist 1 3     -> 1) "b" 2) "c" 3) "d"
LRANGE mylist -3 -1   -> 1) "c" 2) "d" 3) "e"
```

### LSET

Sets the element at the specified index.

```rust
/// LSET key index value
fn cmd_lset(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    match self.storage.lset(&key, index, value) {
        Ok(()) => RespValue::ok(),
        Err(e) => RespValue::error(e),
    }
}
```

### LREM

Removes elements equal to the given value from a list.

- `count > 0`: Remove `count` elements from head to tail
- `count < 0`: Remove `|count|` elements from tail to head  
- `count = 0`: Remove all elements equal to value

```rust
/// LREM key count value
fn cmd_lrem(&self, args: &[RespValue]) -> RespValue {
    // ... validation ...
    let removed = self.storage.lrem(&key, count, &value);
    RespValue::integer(removed as i64)
}
```

**Example:**
```
RPUSH mylist "a" "b" "a" "c" "a"
LREM mylist 2 "a"     -> (integer) 2
LRANGE mylist 0 -1    -> 1) "b" 2) "c" 3) "a"
```

---

## 7. Key Commands

### EXPIRE

```rust
fn cmd_expire(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 2 {
        return RespValue::error("ERR wrong number of arguments for 'EXPIRE' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    let seconds = match self.get_integer(&args[1]) {
        Some(s) => s,
        None => return RespValue::error("ERR value is not an integer"),
    };

    if seconds <= 0 {
        // Non-positive TTL deletes the key
        if self.storage.delete(&key) {
            return RespValue::integer(1);
        }
        return RespValue::integer(0);
    }

    if self.storage.expire(&key, Duration::from_secs(seconds as u64)) {
        RespValue::integer(1)
    } else {
        RespValue::integer(0)
    }
}
```

### TTL

```rust
fn cmd_ttl(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 1 {
        return RespValue::error("ERR wrong number of arguments for 'TTL' command");
    }

    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };

    match self.storage.ttl(&key) {
        Some(ttl) => RespValue::integer(ttl),
        None => RespValue::integer(-2),  // Key doesn't exist
    }
}
```

### KEYS (Pattern Matching)

```rust
fn cmd_keys(&self, args: &[RespValue]) -> RespValue {
    if args.len() != 1 {
        return RespValue::error("ERR wrong number of arguments for 'KEYS' command");
    }

    let pattern = match self.get_string(&args[0]) {
        Some(p) => p,
        None => return RespValue::error("ERR invalid pattern"),
    };

    let keys = self.storage.keys(&pattern);
    let values: Vec<RespValue> = keys.into_iter().map(RespValue::bulk_string).collect();

    RespValue::array(values)
}
```

---

## 8. Server Commands

### PING

```rust
fn cmd_ping(&self, args: &[RespValue]) -> RespValue {
    if args.is_empty() {
        RespValue::pong()
    } else {
        match self.get_bytes(&args[0]) {
            Some(msg) => RespValue::bulk_string(msg),
            None => RespValue::pong(),
        }
    }
}
```

### INFO

```rust
fn cmd_info(&self, _args: &[RespValue]) -> RespValue {
    let stats = self.storage.stats();
    let mem = self.storage.memory_info();
    let uptime = self.start_time.elapsed().as_secs();

    let info = format!(
        "# Server\r\n\
         flashkv_version:0.1.0\r\n\
         uptime_in_seconds:{}\r\n\
         \r\n\
         # Stats\r\n\
         total_commands_processed:{}\r\n\
         \r\n\
         # Keyspace\r\n\
         db0:keys={}\r\n\
         \r\n\
         # Memory\r\n\
         used_memory:{}\r\n",
        uptime,
        stats.get_ops + stats.set_ops + stats.del_ops,
        stats.keys,
        mem.used_memory,
    );

    RespValue::bulk_string(Bytes::from(info))
}
```

### DBSIZE

```rust
fn cmd_dbsize(&self, _args: &[RespValue]) -> RespValue {
    RespValue::integer(self.storage.len() as i64)
}
```

### FLUSHDB

```rust
fn cmd_flushdb(&self, _args: &[RespValue]) -> RespValue {
    self.storage.flush();
    RespValue::ok()
}
```

---

## 9. Adding New Commands

### Step-by-Step Guide

1. **Add the match arm in dispatch()**:
```rust
"MYNEWCMD" => self.cmd_mynewcmd(args),
```

2. **Implement the handler**:
```rust
fn cmd_mynewcmd(&self, args: &[RespValue]) -> RespValue {
    // Validate argument count
    if args.len() != 2 {
        return RespValue::error("ERR wrong number of arguments for 'MYNEWCMD' command");
    }
    
    // Extract arguments
    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };
    
    // Perform operation
    // ...
    
    // Return response
    RespValue::ok()
}
```

3. **Add to COMMAND list** (optional):
```rust
fn cmd_command(&self, _args: &[RespValue]) -> RespValue {
    let commands = vec![
        // ... existing commands ...
        "MYNEWCMD",
    ];
    // ...
}
```

4. **Write tests**:
```rust
#[test]
fn test_mynewcmd() {
    let handler = create_handler();
    
    let response = handler.execute(make_command(&["MYNEWCMD", "arg1", "arg2"]));
    assert_eq!(response, RespValue::ok());
}
```

---

## 10. Error Handling

### Error Response Format

All errors return `RespValue::Error`:

```rust
RespValue::error("ERR wrong number of arguments for 'SET' command")
RespValue::error("ERR value is not an integer or out of range")
RespValue::error("ERR no such key")
```

### Common Error Patterns

```rust
// Wrong argument count
if args.len() != 2 {
    return RespValue::error("ERR wrong number of arguments for 'XXX' command");
}

// Invalid key
let key = match self.get_bytes(&args[0]) {
    Some(k) => k,
    None => return RespValue::error("ERR invalid key"),
};

// Invalid integer
let n = match self.get_integer(&args[1]) {
    Some(n) => n,
    None => return RespValue::error("ERR value is not an integer"),
};

// Storage errors
match self.storage.incr(&key) {
    Ok(n) => RespValue::integer(n),
    Err(e) => RespValue::error(format!("ERR {}", e)),
}
```

### Error Types by Convention

| Prefix | Meaning |
|--------|---------|
| `ERR` | Generic error |
| `WRONGTYPE` | Operation on wrong type |
| `NOAUTH` | Authentication required |
| `NOSCRIPT` | Script not found |
| `LOADING` | Database loading |

---

## 11. Exercises

### Exercise 1: Add GETRANGE Command

Implement `GETRANGE key start end`:

```rust
fn cmd_getrange(&self, args: &[RespValue]) -> RespValue {
    // Get substring from start to end (inclusive)
    // Handle negative indices (from end)
}
```

### Exercise 2: Add SETRANGE Command

Implement `SETRANGE key offset value`:

```rust
fn cmd_setrange(&self, args: &[RespValue]) -> RespValue {
    // Overwrite part of string starting at offset
    // Pad with zeros if offset > length
    // Return new length
}
```

### Exercise 3: Add OBJECT Command

Implement `OBJECT ENCODING key`:

```rust
fn cmd_object(&self, args: &[RespValue]) -> RespValue {
    // OBJECT ENCODING key -> "embstr", "raw", or "int"
    // OBJECT REFCOUNT key -> reference count
    // OBJECT IDLETIME key -> seconds since last access
}
```

### Exercise 4: Add DUMP/RESTORE Commands

Implement serialization:

```rust
fn cmd_dump(&self, args: &[RespValue]) -> RespValue {
    // Serialize key's value to a binary format
}

fn cmd_restore(&self, args: &[RespValue]) -> RespValue {
    // Restore a key from serialized data
}
```

### Exercise 5: Add TOUCH Command

Implement `TOUCH key [key ...]`:

```rust
fn cmd_touch(&self, args: &[RespValue]) -> RespValue {
    // Update last access time for given keys
    // Return count of existing keys
}
```

---

## Key Takeaways

1. **Pattern matching dispatch** - Match command names to handlers
2. **Helper methods** - Normalize input extraction
3. **Consistent error handling** - Always validate and return clear errors
4. **Separation of concerns** - Handler just orchestrates, storage does the work
5. **Return values** - Match Redis conventions exactly

---

## Next Steps

Now let's look at how connections are handled:

**[11_CONNECTION_HANDLER.md](./11_CONNECTION_HANDLER.md)** - Deep dive into `src/connection/handler.rs`!