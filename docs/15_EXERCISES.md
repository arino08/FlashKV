# Hands-On Exercises 🛠️

This document contains comprehensive exercises to extend and deepen your understanding of FlashKV. Each exercise builds on what you've learned.

---

## Table of Contents

1. [Beginner Exercises](#beginner-exercises)
2. [Intermediate Exercises](#intermediate-exercises)
3. [Advanced Exercises](#advanced-exercises)
4. [Project Ideas](#project-ideas)
5. [Solutions](#solutions)

---

## Beginner Exercises

### Exercise 1: Add ECHO Command

**Difficulty**: ⭐

The `ECHO` command already exists, but let's trace through it to understand the flow.

**Task**: Trace a command from client to response:
1. What bytes does `redis-cli` send for `ECHO hello`?
2. How does the parser convert these bytes to `RespValue`?
3. How does the command handler process it?
4. What bytes are sent back?

**Test Your Understanding**:
```bash
# Connect and trace
redis-cli -p 6379
> ECHO "Hello, FlashKV!"
```

---

### Exercise 2: Add DBINFO Command

**Difficulty**: ⭐

Create a new command `DBINFO` that returns basic statistics.

**Task**: Add to `commands/handler.rs`:
```rust
fn cmd_dbinfo(&self, _args: &[RespValue]) -> RespValue {
    // Return a simple string with key count and uptime
    // Example: "Keys: 42, Uptime: 3600s"
}
```

**Steps**:
1. Add the match arm in `dispatch()`
2. Implement the handler
3. Test with `redis-cli`

---

### Exercise 3: Add RANDOMKEY Command

**Difficulty**: ⭐⭐

Implement `RANDOMKEY` that returns a random key from the database.

**Task**: 
1. Add `random_key()` method to `StorageEngine`
2. Add `cmd_randomkey()` to `CommandHandler`
3. Handle the case when database is empty

**Hint**: You'll need to iterate shards and pick a random entry.

---

### Exercise 4: Add OBJECT ENCODING Command

**Difficulty**: ⭐⭐

Implement `OBJECT ENCODING key` that returns the internal encoding of a value.

**Task**: Return one of:
- `"embstr"` for strings ≤ 44 bytes
- `"raw"` for strings > 44 bytes
- `"int"` for integer strings

```rust
fn cmd_object(&self, args: &[RespValue]) -> RespValue {
    // Parse subcommand (ENCODING, REFCOUNT, etc.)
    // For ENCODING, check the value type
}
```

---

### Exercise 5: Inline Command Support

**Difficulty**: ⭐⭐

Redis accepts simple text commands like `PING` instead of `*1\r\n$4\r\nPING\r\n`.

**Task**: Modify the parser to detect and handle inline commands:
- If first byte is not a RESP prefix (`+`, `-`, `:`, `$`, `*`)
- Treat the line as space-separated command and arguments
- Convert to array of bulk strings

**Example**:
```
SET name Ariz\r\n
→ *3\r\n$3\r\nSET\r\n$4\r\nname\r\n$4\r\nAriz\r\n
```

---

## Intermediate Exercises

### Exercise 6: Add GETEX Command

**Difficulty**: ⭐⭐⭐

Implement `GETEX` - get a value and optionally set a new expiry atomically.

**Syntax**:
```
GETEX key [EX seconds | PX milliseconds | EXAT timestamp | PXAT timestamp | PERSIST]
```

**Task**:
1. Add `getex()` to `StorageEngine` that atomically:
   - Gets the value
   - Updates expiry (or removes it with PERSIST)
2. Add command handler
3. Test all variations

---

### Exercise 7: Add COPY Command

**Difficulty**: ⭐⭐⭐

Implement `COPY source destination [REPLACE]`.

**Task**:
1. Copy value from source key to destination
2. If REPLACE is specified, overwrite existing destination
3. Preserve TTL from source key
4. Return 1 on success, 0 if source doesn't exist

**Challenge**: Handle the case where source and destination are in different shards (requires locking both).

---

### Exercise 8: Implement SCAN Command

**Difficulty**: ⭐⭐⭐

`KEYS *` is dangerous on large databases. Implement cursor-based `SCAN`.

**Syntax**:
```
SCAN cursor [MATCH pattern] [COUNT count]
```

**Task**:
1. Add `scan()` to `StorageEngine`:
```rust
pub fn scan(&self, cursor: u64, pattern: Option<&str>, count: usize) 
    -> (u64, Vec<Bytes>)
```
2. Cursor encodes: (shard_index << 32) | position_in_shard
3. Return next cursor and batch of keys
4. Cursor 0 means start, returned 0 means done

**Example**:
```
SCAN 0 MATCH user:* COUNT 10
→ (12345, ["user:1", "user:2", ...])
```

---

### Exercise 9: Add Hash Data Type

**Difficulty**: ⭐⭐⭐⭐

Add support for Redis hashes (HSET, HGET, HDEL, HGETALL).

**Task**:
1. Create a new `ValueType` enum:
```rust
enum ValueType {
    String(Bytes),
    Hash(HashMap<Bytes, Bytes>),
}
```
2. Update `Entry` to use `ValueType`
3. Implement:
   - `HSET key field value`
   - `HGET key field`
   - `HDEL key field`
   - `HGETALL key`
4. Return WRONGTYPE error when mixing types

---

### Exercise 10: Connection Timeout

**Difficulty**: ⭐⭐⭐

Add idle connection timeout - disconnect clients who don't send commands for N seconds.

**Task**:
1. Add `--timeout` command-line argument
2. Track last activity time per connection
3. Use `tokio::time::timeout` in the read loop
4. Clean up and log when timing out

---

## Advanced Exercises

### Exercise 11: Pub/Sub System

**Difficulty**: ⭐⭐⭐⭐

Implement Redis Pub/Sub: SUBSCRIBE, PUBLISH, UNSUBSCRIBE.

**Architecture**:
```rust
struct PubSub {
    // channel -> list of subscriber senders
    channels: RwLock<HashMap<String, Vec<mpsc::Sender<Message>>>>,
}
```

**Task**:
1. Create `src/pubsub.rs` with the subscription manager
2. Handle SUBSCRIBE - client enters subscription mode
3. Handle PUBLISH - broadcast to all subscribers
4. Handle UNSUBSCRIBE - remove from channel
5. Test with two `redis-cli` instances

---

### Exercise 12: Transactions (MULTI/EXEC)

**Difficulty**: ⭐⭐⭐⭐

Implement basic transactions: MULTI, EXEC, DISCARD.

**Task**:
1. Add transaction state to connection handler:
```rust
struct TransactionState {
    active: bool,
    queue: Vec<RespValue>,
}
```
2. MULTI - start queuing commands
3. Commands after MULTI return QUEUED
4. EXEC - execute all queued commands atomically
5. DISCARD - clear queue and exit transaction mode

**Challenge**: How do you ensure atomicity across shards?

---

### Exercise 13: Lua Scripting (EVAL)

**Difficulty**: ⭐⭐⭐⭐⭐

Implement basic Lua script execution.

**Dependencies**: Add `mlua` crate

**Task**:
1. Implement `EVAL script numkeys key... arg...`
2. Expose `redis.call()` and `redis.pcall()` to Lua
3. Handle KEYS and ARGV arrays
4. Return Lua values as RESP

**Example**:
```
EVAL "return redis.call('GET', KEYS[1])" 1 mykey
```

---

### Exercise 14: Persistence (RDB Snapshots)

**Difficulty**: ⭐⭐⭐⭐⭐

Implement Redis-style RDB persistence.

**Task**:
1. Implement `SAVE` command (blocking)
2. Implement `BGSAVE` command (background)
3. Load snapshot on startup
4. RDB format (simplified):
   - Magic number
   - For each key: type, key length, key, value length, value, optional TTL

**File Structure**:
```
[FLASHKV][version][num_keys]
[type][key_len][key][val_len][value][ttl_or_0]
...
[EOF marker]
```

---

### Exercise 15: Cluster Mode (Basic)

**Difficulty**: ⭐⭐⭐⭐⭐

Implement basic cluster awareness.

**Task**:
1. Hash slots: 16384 slots, CRC16 % 16384
2. Each node owns a range of slots
3. Implement `CLUSTER SLOTS` command
4. Return `-MOVED slot ip:port` for keys on other nodes
5. Test with multiple FlashKV instances

---

## Project Ideas

### Project 1: FlashKV CLI Client

Build your own `redis-cli` in Rust:
- Interactive mode
- Command history
- Syntax highlighting
- Auto-completion

### Project 2: FlashKV Benchmark Tool

Create a benchmarking tool:
- Configurable workload (read/write ratio)
- Multiple threads
- Measure latency percentiles (p50, p99, p999)
- Compare with Redis

### Project 3: FlashKV Web Dashboard

Build a web interface:
- Real-time statistics
- Key browser
- Memory graphs
- Slow log viewer

### Project 4: FlashKV Proxy

Build a connection pooling proxy:
- Accept many clients
- Maintain pool of FlashKV connections
- Route commands
- Add authentication

### Project 5: FlashKV + RustyLoad Integration

If you built RustyLoad:
1. Create HTTP endpoints that use FlashKV
2. Load test with RustyLoad
3. Measure throughput and latency
4. Profile and optimize

---

## Solutions

<details>
<summary>Exercise 2: DBINFO Command</summary>

```rust
// In commands/handler.rs

// Add to dispatch():
"DBINFO" => self.cmd_dbinfo(args),

// Add the handler:
fn cmd_dbinfo(&self, _args: &[RespValue]) -> RespValue {
    let stats = self.storage.stats();
    let uptime = self.start_time.elapsed().as_secs();
    
    let info = format!(
        "keys:{}\r\nuptime_seconds:{}\r\nget_ops:{}\r\nset_ops:{}",
        stats.keys, uptime, stats.get_ops, stats.set_ops
    );
    
    RespValue::bulk_string(Bytes::from(info))
}
```
</details>

<details>
<summary>Exercise 3: RANDOMKEY Command</summary>

```rust
// In storage/engine.rs
use rand::Rng;

pub fn random_key(&self) -> Option<Bytes> {
    let mut rng = rand::thread_rng();
    
    // Collect all non-empty shards
    let non_empty_shards: Vec<usize> = self.shards
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.data.read().unwrap().is_empty())
        .map(|(i, _)| i)
        .collect();
    
    if non_empty_shards.is_empty() {
        return None;
    }
    
    // Pick random shard
    let shard_idx = non_empty_shards[rng.gen_range(0..non_empty_shards.len())];
    let shard = &self.shards[shard_idx];
    let data = shard.data.read().unwrap();
    
    // Pick random key from shard
    let keys: Vec<_> = data.keys().collect();
    if keys.is_empty() {
        return None;
    }
    
    let key = keys[rng.gen_range(0..keys.len())];
    Some(key.clone())
}

// In commands/handler.rs
fn cmd_randomkey(&self, _args: &[RespValue]) -> RespValue {
    match self.storage.random_key() {
        Some(key) => RespValue::bulk_string(key),
        None => RespValue::null(),
    }
}
```
</details>

<details>
<summary>Exercise 5: Inline Command Support</summary>

```rust
// In protocol/parser.rs

impl RespParser {
    fn is_resp_prefix(byte: u8) -> bool {
        matches!(byte, b'+' | b'-' | b':' | b'$' | b'*')
    }
    
    fn parse_inline(&self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        // Find end of line
        let end = match find_crlf(buf) {
            Some(pos) => pos,
            None => return Ok(None),
        };
        
        // Parse the line
        let line = match std::str::from_utf8(&buf[..end]) {
            Ok(s) => s,
            Err(e) => return Err(ParseError::InvalidUtf8(e.to_string())),
        };
        
        // Split by whitespace
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() {
            return Err(ParseError::ProtocolError("empty command".to_string()));
        }
        
        // Convert to array of bulk strings
        let elements: Vec<RespValue> = parts
            .into_iter()
            .map(|s| RespValue::BulkString(Bytes::from(s.to_string())))
            .collect();
        
        Ok(Some((RespValue::Array(elements), end + 2)))
    }
    
    fn parse_value(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        if buf.is_empty() {
            return Ok(None);
        }
        
        if !Self::is_resp_prefix(buf[0]) {
            return self.parse_inline(buf);
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
}
```
</details>

<details>
<summary>Exercise 6: GETEX Command</summary>

```rust
// In storage/engine.rs
pub fn getex(&self, key: &Bytes, expiry: Option<Expiry>) -> Option<Bytes> {
    let shard = self.get_shard(key);
    let mut data = shard.data.write().unwrap();
    
    if let Some(entry) = data.get_mut(key) {
        if entry.is_expired() {
            data.remove(key);
            self.key_count.fetch_sub(1, Ordering::Relaxed);
            self.expired_count.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        
        // Update expiry based on option
        match expiry {
            Some(Expiry::Ex(secs)) => {
                entry.expires_at = Some(Instant::now() + Duration::from_secs(secs));
            }
            Some(Expiry::Px(ms)) => {
                entry.expires_at = Some(Instant::now() + Duration::from_millis(ms));
            }
            Some(Expiry::Persist) => {
                entry.expires_at = None;
            }
            None => {}
        }
        
        Some(entry.value.clone())
    } else {
        None
    }
}

pub enum Expiry {
    Ex(u64),      // seconds
    Px(u64),      // milliseconds
    Persist,      // remove expiry
}

// In commands/handler.rs
fn cmd_getex(&self, args: &[RespValue]) -> RespValue {
    if args.is_empty() {
        return RespValue::error("ERR wrong number of arguments for 'GETEX' command");
    }
    
    let key = match self.get_bytes(&args[0]) {
        Some(k) => k,
        None => return RespValue::error("ERR invalid key"),
    };
    
    let expiry = if args.len() > 1 {
        let opt = self.get_string(&args[1]).map(|s| s.to_uppercase());
        match opt.as_deref() {
            Some("EX") if args.len() > 2 => {
                self.get_integer(&args[2]).map(|n| Expiry::Ex(n as u64))
            }
            Some("PX") if args.len() > 2 => {
                self.get_integer(&args[2]).map(|n| Expiry::Px(n as u64))
            }
            Some("PERSIST") => Some(Expiry::Persist),
            _ => None,
        }
    } else {
        None
    };
    
    match self.storage.getex(&key, expiry) {
        Some(value) => RespValue::bulk_string(value),
        None => RespValue::null(),
    }
}
```
</details>

<details>
<summary>Exercise 8: SCAN Command</summary>

```rust
// In storage/engine.rs

pub fn scan(
    &self,
    cursor: u64,
    pattern: Option<&str>,
    count: usize,
) -> (u64, Vec<Bytes>) {
    let count = count.max(10);  // Minimum 10
    let glob = pattern.map(|p| GlobPattern::new(p));
    
    // Decode cursor: high 32 bits = shard, low 32 bits = position
    let start_shard = (cursor >> 32) as usize;
    let start_pos = (cursor & 0xFFFFFFFF) as usize;
    
    let mut results = Vec::new();
    let mut current_shard = start_shard;
    let mut current_pos = start_pos;
    
    while results.len() < count && current_shard < NUM_SHARDS {
        let shard = &self.shards[current_shard];
        let data = shard.data.read().unwrap();
        
        for (i, (key, entry)) in data.iter().enumerate() {
            // Skip entries before our position in first shard
            if current_shard == start_shard && i < current_pos {
                continue;
            }
            
            if entry.is_expired() {
                continue;
            }
            
            // Check pattern match
            let matches = match (&glob, std::str::from_utf8(key)) {
                (Some(g), Ok(k)) => g.matches(k),
                (Some(_), Err(_)) => false,
                (None, _) => true,
            };
            
            if matches {
                results.push(key.clone());
                
                if results.len() >= count {
                    // Return cursor pointing to next position
                    let next_cursor = ((current_shard as u64) << 32) | ((i + 1) as u64);
                    return (next_cursor, results);
                }
            }
        }
        
        // Move to next shard
        current_shard += 1;
        current_pos = 0;
    }
    
    // Done - return cursor 0
    (0, results)
}

// In commands/handler.rs
fn cmd_scan(&self, args: &[RespValue]) -> RespValue {
    if args.is_empty() {
        return RespValue::error("ERR wrong number of arguments for 'SCAN' command");
    }
    
    let cursor = match self.get_integer(&args[0]) {
        Some(c) if c >= 0 => c as u64,
        _ => return RespValue::error("ERR invalid cursor"),
    };
    
    let mut pattern: Option<String> = None;
    let mut count: usize = 10;
    
    let mut i = 1;
    while i < args.len() {
        let opt = self.get_string(&args[i]).map(|s| s.to_uppercase());
        match opt.as_deref() {
            Some("MATCH") if i + 1 < args.len() => {
                pattern = self.get_string(&args[i + 1]);
                i += 2;
            }
            Some("COUNT") if i + 1 < args.len() => {
                count = self.get_integer(&args[i + 1]).unwrap_or(10) as usize;
                i += 2;
            }
            _ => i += 1,
        }
    }
    
    let (next_cursor, keys) = self.storage.scan(
        cursor,
        pattern.as_deref(),
        count,
    );
    
    let keys_array: Vec<RespValue> = keys
        .into_iter()
        .map(RespValue::bulk_string)
        .collect();
    
    RespValue::array(vec![
        RespValue::bulk_string(Bytes::from(next_cursor.to_string())),
        RespValue::array(keys_array),
    ])
}
```
</details>

---

## Tips for Success

1. **Start small**: Get basic functionality working, then add features
2. **Write tests first**: Helps you think through edge cases
3. **Use `redis-cli`**: Test compatibility with real Redis client
4. **Read Redis docs**: Understand expected behavior
5. **Profile before optimizing**: Use `cargo flamegraph` to find bottlenecks
6. **Ask for help**: If stuck, check Redis source code for reference

---

## What's Next?

After completing these exercises, you'll have:
- Deep understanding of database internals
- Experience with concurrent programming
- A portfolio project that stands out
- Skills applicable to real-world systems

Consider:
- Contributing to open-source Rust projects
- Writing a blog post about what you learned
- Building your own variations (time-series DB, graph DB, etc.)

---

**Happy coding! 🚀**