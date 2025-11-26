//! Command Handler Module
//!
//! This module implements all the Redis-compatible commands for FlashKV.
//! It parses incoming RESP arrays and dispatches them to the appropriate handlers.
//!
//! ## Supported Commands
//!
//! ### String Commands
//! - `SET key value [EX seconds | PX milliseconds]` - Set a key
//! - `GET key` - Get a key's value
//! - `DEL key [key ...]` - Delete keys
//! - `EXISTS key [key ...]` - Check if keys exist
//! - `APPEND key value` - Append to a string
//! - `STRLEN key` - Get string length
//! - `INCR key` - Increment integer
//! - `INCRBY key increment` - Increment by amount
//! - `DECR key` - Decrement integer
//! - `DECRBY key decrement` - Decrement by amount
//! - `MSET key value [key value ...]` - Set multiple keys
//! - `MGET key [key ...]` - Get multiple keys
//! - `SETNX key value` - Set if not exists
//! - `SETEX key seconds value` - Set with expiry
//! - `GETSET key value` - Set and return old value
//!
//! ### List Commands
//! - `LPUSH key value [value ...]` - Push values to the head of a list
//! - `RPUSH key value [value ...]` - Push values to the tail of a list
//! - `LPOP key` - Remove and return the first element
//! - `RPOP key` - Remove and return the last element
//! - `LLEN key` - Get the length of a list
//! - `LINDEX key index` - Get element at index
//! - `LRANGE key start stop` - Get a range of elements
//! - `LSET key index value` - Set element at index
//! - `LREM key count value` - Remove elements equal to value
//!
//! ### Key Commands
//! - `EXPIRE key seconds` - Set expiry
//! - `PEXPIRE key milliseconds` - Set expiry in ms
//! - `TTL key` - Get remaining TTL
//! - `PTTL key` - Get remaining TTL in ms
//! - `PERSIST key` - Remove expiry
//! - `KEYS pattern` - Find keys by pattern
//! - `TYPE key` - Get key type ("string", "list", or "none")
//! - `RENAME key newkey` - Rename a key
//! - `RENAMENX key newkey` - Rename if new key doesn't exist
//!
//! ### Server Commands
//! - `PING [message]` - Test connection
//! - `ECHO message` - Echo message
//! - `INFO [section]` - Server information
//! - `DBSIZE` - Number of keys
//! - `FLUSHDB` - Clear database
//! - `COMMAND` - List commands
//! - `CONFIG GET parameter` - Get config
//! - `TIME` - Server time
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     CommandHandler                          │
//! │                                                             │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
//! │  │   parse()   │───>│  dispatch() │───>│  execute()  │     │
//! │  └─────────────┘    └─────────────┘    └─────────────┘     │
//! │                                               │             │
//! │                                               ▼             │
//! │                                      StorageEngine          │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::protocol::RespValue;
use crate::storage::StorageEngine;
use bytes::Bytes;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Handles Redis commands by dispatching them to the appropriate handlers.
#[derive(Clone)]
pub struct CommandHandler {
    /// The storage engine
    storage: Arc<StorageEngine>,
    /// Server start time for INFO command
    start_time: std::time::Instant,
}

impl CommandHandler {
    /// Creates a new command handler with the given storage engine.
    pub fn new(storage: Arc<StorageEngine>) -> Self {
        Self {
            storage,
            start_time: std::time::Instant::now(),
        }
    }

    /// Executes a command and returns the response.
    ///
    /// # Arguments
    ///
    /// * `command` - The parsed RESP value (should be an array)
    ///
    /// # Returns
    ///
    /// The RESP response to send back to the client.
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

    /// Dispatches a command to its handler.
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
            "DEBUG" => self.cmd_debug(args),
            "QUIT" => RespValue::ok(),

            // Unknown command
            _ => RespValue::error(format!("ERR unknown command '{}'", cmd)),
        }
    }

    // ========================================================================
    // Helper functions
    // ========================================================================

    /// Extracts a Bytes value from a RespValue.
    fn get_bytes(&self, value: &RespValue) -> Option<Bytes> {
        match value {
            RespValue::BulkString(b) => Some(b.clone()),
            RespValue::SimpleString(s) => Some(Bytes::from(s.clone())),
            _ => None,
        }
    }

    /// Extracts a string from a RespValue.
    fn get_string(&self, value: &RespValue) -> Option<String> {
        match value {
            RespValue::BulkString(b) => std::str::from_utf8(b).ok().map(|s| s.to_string()),
            RespValue::SimpleString(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Extracts an integer from a RespValue.
    fn get_integer(&self, value: &RespValue) -> Option<i64> {
        match value {
            RespValue::Integer(n) => Some(*n),
            RespValue::BulkString(b) => std::str::from_utf8(b).ok().and_then(|s| s.parse().ok()),
            RespValue::SimpleString(s) => s.parse().ok(),
            _ => None,
        }
    }

    // ========================================================================
    // String Commands
    // ========================================================================

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
        let mut nx = false; // Only set if not exists
        let mut xx = false; // Only set if exists
        let mut get = false; // Return old value

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
                "GET" => get = true,
                "KEEPTTL" => {
                    // Keep existing TTL - we'd need to implement this
                }
                _ => return RespValue::error(format!("ERR unknown option '{}'", opt)),
            }
            i += 1;
        }

        // Handle NX/XX conditions
        let exists = self.storage.exists(&key);

        if nx && exists {
            return if get {
                match self.storage.get(&key) {
                    Some(v) => RespValue::bulk_string(v),
                    None => RespValue::null(),
                }
            } else {
                RespValue::null()
            };
        }

        if xx && !exists {
            return RespValue::null();
        }

        // Get old value if GET option is specified
        let old_value = if get { self.storage.get(&key) } else { None };

        // Perform the SET
        match ttl {
            Some(duration) => self.storage.set_with_ttl(key, value, duration),
            None => self.storage.set(key, value),
        };

        if get {
            match old_value {
                Some(v) => RespValue::bulk_string(v),
                None => RespValue::null(),
            }
        } else {
            RespValue::ok()
        }
    }

    /// GET key
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

    /// DEL key [key ...]
    fn cmd_del(&self, args: &[RespValue]) -> RespValue {
        if args.is_empty() {
            return RespValue::error("ERR wrong number of arguments for 'DEL' command");
        }

        let keys: Vec<Bytes> = args.iter().filter_map(|a| self.get_bytes(a)).collect();

        let deleted = self.storage.delete_many(&keys);
        RespValue::integer(deleted as i64)
    }

    /// EXISTS key [key ...]
    fn cmd_exists(&self, args: &[RespValue]) -> RespValue {
        if args.is_empty() {
            return RespValue::error("ERR wrong number of arguments for 'EXISTS' command");
        }

        let keys: Vec<Bytes> = args.iter().filter_map(|a| self.get_bytes(a)).collect();

        let count = self.storage.exists_many(&keys);
        RespValue::integer(count as i64)
    }

    /// APPEND key value
    fn cmd_append(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'APPEND' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let value = match self.get_bytes(&args[1]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        let new_len = self.storage.append(&key, &value);
        RespValue::integer(new_len as i64)
    }

    /// STRLEN key
    fn cmd_strlen(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'STRLEN' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let len = self.storage.strlen(&key);
        RespValue::integer(len as i64)
    }

    /// INCR key
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

    /// INCRBY key increment
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

    /// DECR key
    fn cmd_decr(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'DECR' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        match self.storage.decr(&key) {
            Ok(n) => RespValue::integer(n),
            Err(e) => RespValue::error(format!("ERR {}", e)),
        }
    }

    /// DECRBY key decrement
    fn cmd_decrby(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'DECRBY' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let delta = match self.get_integer(&args[1]) {
            Some(d) => d,
            None => return RespValue::error("ERR value is not an integer"),
        };

        match self.storage.decr_by(&key, delta) {
            Ok(n) => RespValue::integer(n),
            Err(e) => RespValue::error(format!("ERR {}", e)),
        }
    }

    /// MSET key value [key value ...]
    fn cmd_mset(&self, args: &[RespValue]) -> RespValue {
        if args.is_empty() || args.len() % 2 != 0 {
            return RespValue::error("ERR wrong number of arguments for 'MSET' command");
        }

        for i in (0..args.len()).step_by(2) {
            let key = match self.get_bytes(&args[i]) {
                Some(k) => k,
                None => return RespValue::error("ERR invalid key"),
            };

            let value = match self.get_bytes(&args[i + 1]) {
                Some(v) => v,
                None => return RespValue::error("ERR invalid value"),
            };

            self.storage.set(key, value);
        }

        RespValue::ok()
    }

    /// MGET key [key ...]
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

    /// SETNX key value
    fn cmd_setnx(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'SETNX' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let value = match self.get_bytes(&args[1]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        if self.storage.exists(&key) {
            RespValue::integer(0)
        } else {
            self.storage.set(key, value);
            RespValue::integer(1)
        }
    }

    /// SETEX key seconds value
    fn cmd_setex(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 3 {
            return RespValue::error("ERR wrong number of arguments for 'SETEX' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let seconds = match self.get_integer(&args[1]) {
            Some(s) if s > 0 => s as u64,
            _ => return RespValue::error("ERR invalid expire time"),
        };

        let value = match self.get_bytes(&args[2]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        self.storage
            .set_with_ttl(key, value, Duration::from_secs(seconds));
        RespValue::ok()
    }

    /// PSETEX key milliseconds value
    fn cmd_psetex(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 3 {
            return RespValue::error("ERR wrong number of arguments for 'PSETEX' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let ms = match self.get_integer(&args[1]) {
            Some(m) if m > 0 => m as u64,
            _ => return RespValue::error("ERR invalid expire time"),
        };

        let value = match self.get_bytes(&args[2]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        self.storage
            .set_with_ttl(key, value, Duration::from_millis(ms));
        RespValue::ok()
    }

    /// GETSET key value (deprecated but still supported)
    fn cmd_getset(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'GETSET' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let value = match self.get_bytes(&args[1]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        let old_value = self.storage.get(&key);
        self.storage.set(key, value);

        match old_value {
            Some(v) => RespValue::bulk_string(v),
            None => RespValue::null(),
        }
    }

    /// GETDEL key
    fn cmd_getdel(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'GETDEL' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let value = self.storage.get(&key);
        self.storage.delete(&key);

        match value {
            Some(v) => RespValue::bulk_string(v),
            None => RespValue::null(),
        }
    }

    // ========================================================================
    // List Commands
    // ========================================================================

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

    /// RPUSH key value [value ...]
    fn cmd_rpush(&self, args: &[RespValue]) -> RespValue {
        if args.len() < 2 {
            return RespValue::error("ERR wrong number of arguments for 'RPUSH' command");
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

        let len = self.storage.rpush(key, values);
        RespValue::integer(len as i64)
    }

    /// LPOP key
    fn cmd_lpop(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'LPOP' command");
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

        match self.storage.lpop(&key) {
            Some(v) => RespValue::bulk_string(v),
            None => RespValue::null(),
        }
    }

    /// RPOP key
    fn cmd_rpop(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'RPOP' command");
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

        match self.storage.rpop(&key) {
            Some(v) => RespValue::bulk_string(v),
            None => RespValue::null(),
        }
    }

    /// LLEN key
    fn cmd_llen(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'LLEN' command");
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

        let len = self.storage.llen(&key);
        RespValue::integer(len as i64)
    }

    /// LINDEX key index
    fn cmd_lindex(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'LINDEX' command");
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

        let index = match self.get_integer(&args[1]) {
            Some(i) => i,
            None => return RespValue::error("ERR value is not an integer or out of range"),
        };

        match self.storage.lindex(&key, index) {
            Some(v) => RespValue::bulk_string(v),
            None => RespValue::null(),
        }
    }

    /// LRANGE key start stop
    fn cmd_lrange(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 3 {
            return RespValue::error("ERR wrong number of arguments for 'LRANGE' command");
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

        let start = match self.get_integer(&args[1]) {
            Some(i) => i,
            None => return RespValue::error("ERR value is not an integer or out of range"),
        };

        let stop = match self.get_integer(&args[2]) {
            Some(i) => i,
            None => return RespValue::error("ERR value is not an integer or out of range"),
        };

        let elements = self.storage.lrange(&key, start, stop);
        let values: Vec<RespValue> = elements.into_iter().map(RespValue::bulk_string).collect();
        RespValue::array(values)
    }

    /// LSET key index value
    fn cmd_lset(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 3 {
            return RespValue::error("ERR wrong number of arguments for 'LSET' command");
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

        let index = match self.get_integer(&args[1]) {
            Some(i) => i,
            None => return RespValue::error("ERR value is not an integer or out of range"),
        };

        let value = match self.get_bytes(&args[2]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        match self.storage.lset(&key, index, value) {
            Ok(()) => RespValue::ok(),
            Err(e) => RespValue::error(e),
        }
    }

    /// LREM key count value
    fn cmd_lrem(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 3 {
            return RespValue::error("ERR wrong number of arguments for 'LREM' command");
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

        let count = match self.get_integer(&args[1]) {
            Some(c) => c,
            None => return RespValue::error("ERR value is not an integer or out of range"),
        };

        let value = match self.get_bytes(&args[2]) {
            Some(v) => v,
            None => return RespValue::error("ERR invalid value"),
        };

        let removed = self.storage.lrem(&key, count, &value);
        RespValue::integer(removed as i64)
    }

    // ========================================================================
    // Key Commands
    // ========================================================================

    /// EXPIRE key seconds
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

        if self
            .storage
            .expire(&key, Duration::from_secs(seconds as u64))
        {
            RespValue::integer(1)
        } else {
            RespValue::integer(0)
        }
    }

    /// PEXPIRE key milliseconds
    fn cmd_pexpire(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'PEXPIRE' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let ms = match self.get_integer(&args[1]) {
            Some(m) => m,
            None => return RespValue::error("ERR value is not an integer"),
        };

        if ms <= 0 {
            if self.storage.delete(&key) {
                return RespValue::integer(1);
            }
            return RespValue::integer(0);
        }

        if self.storage.expire(&key, Duration::from_millis(ms as u64)) {
            RespValue::integer(1)
        } else {
            RespValue::integer(0)
        }
    }

    /// EXPIREAT key timestamp
    fn cmd_expireat(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'EXPIREAT' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let timestamp = match self.get_integer(&args[1]) {
            Some(t) => t,
            None => return RespValue::error("ERR value is not an integer"),
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as i64;

        let ttl_secs = timestamp - now;

        if ttl_secs <= 0 {
            if self.storage.delete(&key) {
                return RespValue::integer(1);
            }
            return RespValue::integer(0);
        }

        if self
            .storage
            .expire(&key, Duration::from_secs(ttl_secs as u64))
        {
            RespValue::integer(1)
        } else {
            RespValue::integer(0)
        }
    }

    /// TTL key
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
            None => RespValue::integer(-2), // Key doesn't exist
        }
    }

    /// PTTL key
    fn cmd_pttl(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'PTTL' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        match self.storage.pttl(&key) {
            Some(ttl) => RespValue::integer(ttl),
            None => RespValue::integer(-2),
        }
    }

    /// PERSIST key
    fn cmd_persist(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'PERSIST' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        if self.storage.persist(&key) {
            RespValue::integer(1)
        } else {
            RespValue::integer(0)
        }
    }

    /// KEYS pattern
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

    /// TYPE key
    fn cmd_type(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'TYPE' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        RespValue::simple_string(self.storage.key_type(&key))
    }

    /// RENAME key newkey
    fn cmd_rename(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'RENAME' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let newkey = match self.get_bytes(&args[1]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid new key"),
        };

        // Get the value (and TTL if exists)
        let entry = match self.storage.get_entry(&key) {
            Some(e) => e,
            None => return RespValue::error("ERR no such key"),
        };

        // Delete old key
        self.storage.delete(&key);

        // Set new key with same value and TTL
        if let Some(expires_at) = entry.expires_at {
            let now = std::time::Instant::now();
            if expires_at > now {
                let remaining = expires_at - now;
                self.storage.set_with_ttl(newkey, entry.value, remaining);
            } else {
                // Already expired, don't set
                return RespValue::ok();
            }
        } else {
            self.storage.set(newkey, entry.value);
        }

        RespValue::ok()
    }

    /// RENAMENX key newkey
    fn cmd_renamenx(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 2 {
            return RespValue::error("ERR wrong number of arguments for 'RENAMENX' command");
        }

        let key = match self.get_bytes(&args[0]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid key"),
        };

        let newkey = match self.get_bytes(&args[1]) {
            Some(k) => k,
            None => return RespValue::error("ERR invalid new key"),
        };

        if !self.storage.exists(&key) {
            return RespValue::error("ERR no such key");
        }

        if self.storage.exists(&newkey) {
            return RespValue::integer(0);
        }

        let entry = self.storage.get_entry(&key).unwrap();
        self.storage.delete(&key);

        if let Some(expires_at) = entry.expires_at {
            let now = std::time::Instant::now();
            if expires_at > now {
                let remaining = expires_at - now;
                self.storage.set_with_ttl(newkey, entry.value, remaining);
            }
        } else {
            self.storage.set(newkey, entry.value);
        }

        RespValue::integer(1)
    }

    // ========================================================================
    // Server Commands
    // ========================================================================

    /// PING [message]
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

    /// ECHO message
    fn cmd_echo(&self, args: &[RespValue]) -> RespValue {
        if args.len() != 1 {
            return RespValue::error("ERR wrong number of arguments for 'ECHO' command");
        }

        match self.get_bytes(&args[0]) {
            Some(msg) => RespValue::bulk_string(msg),
            None => RespValue::error("ERR invalid message"),
        }
    }

    /// INFO [section]
    fn cmd_info(&self, _args: &[RespValue]) -> RespValue {
        let stats = self.storage.stats();
        let mem = self.storage.memory_info();
        let uptime = self.start_time.elapsed().as_secs();

        let info = format!(
            "# Server\r\n\
             flashkv_version:0.1.0\r\n\
             rust_version:{}\r\n\
             os:{}\r\n\
             uptime_in_seconds:{}\r\n\
             \r\n\
             # Stats\r\n\
             total_connections_received:0\r\n\
             total_commands_processed:{}\r\n\
             \r\n\
             # Keyspace\r\n\
             db0:keys={},expires=0\r\n\
             \r\n\
             # Memory\r\n\
             used_memory:{}\r\n\
             used_memory_human:{}KB\r\n\
             \r\n\
             # Operations\r\n\
             get_ops:{}\r\n\
             set_ops:{}\r\n\
             del_ops:{}\r\n\
             expired_keys:{}\r\n",
            env!("CARGO_PKG_RUST_VERSION").to_string(),
            std::env::consts::OS,
            uptime,
            stats.get_ops + stats.set_ops + stats.del_ops,
            stats.keys,
            mem.used_memory,
            mem.used_memory / 1024,
            stats.get_ops,
            stats.set_ops,
            stats.del_ops,
            stats.expired,
        );

        RespValue::bulk_string(Bytes::from(info))
    }

    /// DBSIZE
    fn cmd_dbsize(&self, _args: &[RespValue]) -> RespValue {
        RespValue::integer(self.storage.len() as i64)
    }

    /// FLUSHDB / FLUSHALL
    fn cmd_flushdb(&self, _args: &[RespValue]) -> RespValue {
        self.storage.flush();
        RespValue::ok()
    }

    /// COMMAND
    fn cmd_command(&self, _args: &[RespValue]) -> RespValue {
        // Return a simple list of supported commands
        let commands = vec![
            "SET", "GET", "DEL", "EXISTS", "EXPIRE", "TTL", "PTTL", "INCR", "INCRBY", "DECR",
            "DECRBY", "APPEND", "STRLEN", "MSET", "MGET", "SETNX", "SETEX", "PSETEX", "GETSET",
            "PEXPIRE", "PERSIST", "KEYS", "TYPE", "RENAME", "RENAMENX", "PING", "ECHO", "INFO",
            "DBSIZE", "FLUSHDB", "FLUSHALL", "COMMAND", "CONFIG", "TIME", "QUIT", "GETDEL",
            "EXPIREAT",
        ];

        let values: Vec<RespValue> = commands
            .into_iter()
            .map(|c| RespValue::bulk_string(Bytes::from(c)))
            .collect();

        RespValue::array(values)
    }

    /// CONFIG GET parameter
    fn cmd_config(&self, args: &[RespValue]) -> RespValue {
        if args.is_empty() {
            return RespValue::error("ERR wrong number of arguments for 'CONFIG' command");
        }

        let subcommand = match self.get_string(&args[0]) {
            Some(s) => s.to_uppercase(),
            None => return RespValue::error("ERR invalid subcommand"),
        };

        match subcommand.as_str() {
            "GET" => {
                if args.len() < 2 {
                    return RespValue::error("ERR wrong number of arguments for 'CONFIG GET'");
                }
                // Return empty array for most config gets (we don't have config)
                RespValue::array(vec![])
            }
            "SET" => {
                // We don't support config set
                RespValue::ok()
            }
            _ => RespValue::error(format!("ERR unknown CONFIG subcommand '{}'", subcommand)),
        }
    }

    /// TIME
    fn cmd_time(&self, _args: &[RespValue]) -> RespValue {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);

        let secs = now.as_secs().to_string();
        let micros = (now.subsec_micros()).to_string();

        RespValue::array(vec![
            RespValue::bulk_string(Bytes::from(secs)),
            RespValue::bulk_string(Bytes::from(micros)),
        ])
    }

    /// DEBUG commands (for testing)
    fn cmd_debug(&self, args: &[RespValue]) -> RespValue {
        if args.is_empty() {
            return RespValue::error("ERR wrong number of arguments for 'DEBUG' command");
        }

        let subcommand = match self.get_string(&args[0]) {
            Some(s) => s.to_uppercase(),
            None => return RespValue::error("ERR invalid subcommand"),
        };

        match subcommand.as_str() {
            "SLEEP" => {
                if args.len() < 2 {
                    return RespValue::error("ERR wrong number of arguments for 'DEBUG SLEEP'");
                }
                // We don't actually sleep (it would block), just return OK
                RespValue::ok()
            }
            _ => RespValue::error(format!("ERR unknown DEBUG subcommand '{}'", subcommand)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_handler() -> CommandHandler {
        let storage = Arc::new(StorageEngine::new());
        CommandHandler::new(storage)
    }

    fn make_command(args: &[&str]) -> RespValue {
        RespValue::Array(
            args.iter()
                .map(|s| RespValue::bulk_string(Bytes::from(s.to_string())))
                .collect(),
        )
    }

    #[test]
    fn test_ping() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["PING"]));
        assert_eq!(response, RespValue::simple_string("PONG"));

        let response = handler.execute(make_command(&["PING", "hello"]));
        assert_eq!(response, RespValue::bulk_string(Bytes::from("hello")));
    }

    #[test]
    fn test_set_get() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["SET", "key", "value"]));
        assert_eq!(response, RespValue::ok());

        let response = handler.execute(make_command(&["GET", "key"]));
        assert_eq!(response, RespValue::bulk_string(Bytes::from("value")));
    }

    #[test]
    fn test_get_nonexistent() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["GET", "nonexistent"]));
        assert_eq!(response, RespValue::null());
    }

    #[test]
    fn test_del() {
        let handler = create_handler();

        handler.execute(make_command(&["SET", "key1", "value1"]));
        handler.execute(make_command(&["SET", "key2", "value2"]));

        let response = handler.execute(make_command(&["DEL", "key1", "key2", "key3"]));
        assert_eq!(response, RespValue::integer(2));
    }

    #[test]
    fn test_exists() {
        let handler = create_handler();

        handler.execute(make_command(&["SET", "key1", "value1"]));

        let response = handler.execute(make_command(&["EXISTS", "key1"]));
        assert_eq!(response, RespValue::integer(1));

        let response = handler.execute(make_command(&["EXISTS", "nonexistent"]));
        assert_eq!(response, RespValue::integer(0));
    }

    #[test]
    fn test_incr_decr() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["INCR", "counter"]));
        assert_eq!(response, RespValue::integer(1));

        let response = handler.execute(make_command(&["INCR", "counter"]));
        assert_eq!(response, RespValue::integer(2));

        let response = handler.execute(make_command(&["DECR", "counter"]));
        assert_eq!(response, RespValue::integer(1));

        let response = handler.execute(make_command(&["INCRBY", "counter", "10"]));
        assert_eq!(response, RespValue::integer(11));
    }

    #[test]
    fn test_mset_mget() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["MSET", "k1", "v1", "k2", "v2"]));
        assert_eq!(response, RespValue::ok());

        let response = handler.execute(make_command(&["MGET", "k1", "k2", "k3"]));
        assert_eq!(
            response,
            RespValue::Array(vec![
                RespValue::bulk_string(Bytes::from("v1")),
                RespValue::bulk_string(Bytes::from("v2")),
                RespValue::null(),
            ])
        );
    }

    #[test]
    fn test_set_with_options() {
        let handler = create_handler();

        // SET with NX
        let response = handler.execute(make_command(&["SET", "key", "value", "NX"]));
        assert_eq!(response, RespValue::ok());

        // SET with NX on existing key should return nil
        let response = handler.execute(make_command(&["SET", "key", "newvalue", "NX"]));
        assert_eq!(response, RespValue::null());

        // SET with XX on existing key
        let response = handler.execute(make_command(&["SET", "key", "newvalue", "XX"]));
        assert_eq!(response, RespValue::ok());

        // Verify value changed
        let response = handler.execute(make_command(&["GET", "key"]));
        assert_eq!(response, RespValue::bulk_string(Bytes::from("newvalue")));
    }

    #[test]
    fn test_append() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["APPEND", "key", "Hello"]));
        assert_eq!(response, RespValue::integer(5));

        let response = handler.execute(make_command(&["APPEND", "key", " World"]));
        assert_eq!(response, RespValue::integer(11));

        let response = handler.execute(make_command(&["GET", "key"]));
        assert_eq!(response, RespValue::bulk_string(Bytes::from("Hello World")));
    }

    #[test]
    fn test_dbsize() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["DBSIZE"]));
        assert_eq!(response, RespValue::integer(0));

        handler.execute(make_command(&["SET", "key1", "value1"]));
        handler.execute(make_command(&["SET", "key2", "value2"]));

        let response = handler.execute(make_command(&["DBSIZE"]));
        assert_eq!(response, RespValue::integer(2));
    }

    #[test]
    fn test_flushdb() {
        let handler = create_handler();

        handler.execute(make_command(&["SET", "key1", "value1"]));
        handler.execute(make_command(&["SET", "key2", "value2"]));

        let response = handler.execute(make_command(&["FLUSHDB"]));
        assert_eq!(response, RespValue::ok());

        let response = handler.execute(make_command(&["DBSIZE"]));
        assert_eq!(response, RespValue::integer(0));
    }

    #[test]
    fn test_unknown_command() {
        let handler = create_handler();

        let response = handler.execute(make_command(&["UNKNOWN"]));
        assert!(matches!(response, RespValue::Error(_)));
    }
}
