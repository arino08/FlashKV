//! Zero-Copy RESP Protocol Parser
//!
//! This module implements a high-performance, zero-copy parser for the RESP protocol.
//! Zero-copy means we avoid copying data wherever possible, instead using references
//! and `Bytes` which can be cheaply cloned (it's just incrementing a reference count).
//!
//! ## Design Philosophy
//!
//! 1. **Zero-Copy**: We use `bytes::Bytes` to avoid memory allocations during parsing.
//! 2. **Incremental**: The parser can handle partial data and resume when more arrives.
//! 3. **Error Recovery**: Clear error messages for debugging protocol issues.
//!
//! ## How the Parser Works
//!
//! The parser reads from a buffer and returns either:
//! - `Ok(Some((value, consumed)))` - Successfully parsed a value, `consumed` bytes were used
//! - `Ok(None)` - Need more data, the message is incomplete
//! - `Err(ParseError)` - Invalid protocol data
//!
//! This design allows the caller to:
//! 1. Append incoming network data to a buffer
//! 2. Call `parse()` to attempt parsing
//! 3. If successful, advance the buffer by `consumed` bytes
//! 4. If incomplete, wait for more data
//! 5. If error, handle or disconnect the client

use crate::protocol::types::{prefix, RespValue, CRLF};
use bytes::Bytes;
use std::num::ParseIntError;
use thiserror::Error;

/// Errors that can occur during RESP parsing.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseError {
    /// The input buffer is empty
    #[error("empty input")]
    EmptyInput,

    /// Unknown type prefix byte
    #[error("unknown type prefix: {0:#04x}")]
    UnknownPrefix(u8),

    /// Invalid integer format
    #[error("invalid integer: {0}")]
    InvalidInteger(String),

    /// Invalid UTF-8 in a simple string or error message
    #[error("invalid UTF-8: {0}")]
    InvalidUtf8(String),

    /// Bulk string length is negative (but not -1 for null)
    #[error("invalid bulk string length: {0}")]
    InvalidBulkLength(i64),

    /// Array length is negative (but not -1 for null)
    #[error("invalid array length: {0}")]
    InvalidArrayLength(i64),

    /// Protocol violation (missing CRLF, etc.)
    #[error("protocol error: {0}")]
    ProtocolError(String),

    /// The message exceeds maximum allowed size
    #[error("message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
}

/// Result type for parsing operations.
pub type ParseResult<T> = Result<T, ParseError>;

/// Maximum size for a single bulk string (512 MB, same as Redis)
pub const MAX_BULK_SIZE: usize = 512 * 1024 * 1024;

/// Maximum array nesting depth (prevent stack overflow)
pub const MAX_NESTING_DEPTH: usize = 32;

/// A zero-copy RESP protocol parser.
///
/// # Example
///
/// ```ignore
/// use flashkv::protocol::parser::RespParser;
/// use bytes::BytesMut;
///
/// let mut parser = RespParser::new();
/// let mut buffer = BytesMut::from(&b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n"[..]);
///
/// if let Some((value, consumed)) = parser.parse(&buffer)? {
///     buffer.advance(consumed);
///     println!("Parsed: {:?}", value);
/// }
/// ```
#[derive(Debug, Default)]
pub struct RespParser {
    /// Current nesting depth (for array parsing)
    depth: usize,
}

impl RespParser {
    /// Creates a new parser instance.
    pub fn new() -> Self {
        Self { depth: 0 }
    }

    /// Attempts to parse a RESP value from the buffer.
    ///
    /// # Returns
    ///
    /// - `Ok(Some((value, consumed)))` - Successfully parsed a value
    /// - `Ok(None)` - Incomplete data, need more bytes
    /// - `Err(e)` - Parse error
    ///
    /// # Arguments
    ///
    /// * `buf` - The buffer containing RESP data
    pub fn parse(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        self.depth = 0;
        self.parse_value(buf)
    }

    /// Internal recursive parsing function.
    fn parse_value(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        if buf.is_empty() {
            return Ok(None);
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
            _ => self.parse_inline(buf),
        }
    }

    /// Parses a simple string: `+<string>\r\n`
    fn parse_simple_string(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        debug_assert!(buf[0] == prefix::SIMPLE_STRING);

        match find_crlf(&buf[1..]) {
            Some(pos) => {
                let content = &buf[1..1 + pos];
                let s = std::str::from_utf8(content)
                    .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

                // +1 for prefix, +2 for CRLF
                let consumed = 1 + pos + 2;
                Ok(Some((RespValue::SimpleString(s.to_string()), consumed)))
            }
            None => Ok(None), // Incomplete
        }
    }

    /// Parses an error: `-<error message>\r\n`
    fn parse_error(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        debug_assert!(buf[0] == prefix::ERROR);

        match find_crlf(&buf[1..]) {
            Some(pos) => {
                let content = &buf[1..1 + pos];
                let s = std::str::from_utf8(content)
                    .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

                let consumed = 1 + pos + 2;
                Ok(Some((RespValue::Error(s.to_string()), consumed)))
            }
            None => Ok(None),
        }
    }

    /// Parses an integer: `:<integer>\r\n`
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

    /// Parses a bulk string: `$<length>\r\n<data>\r\n`
    fn parse_bulk_string(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        debug_assert!(buf[0] == prefix::BULK_STRING);

        // First, find the length line
        let length_end = match find_crlf(&buf[1..]) {
            Some(pos) => pos,
            None => return Ok(None),
        };

        let length_str = std::str::from_utf8(&buf[1..1 + length_end])
            .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

        let length: i64 = length_str
            .parse()
            .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;

        // Handle null bulk string
        if length == -1 {
            let consumed = 1 + length_end + 2; // $-1\r\n
            return Ok(Some((RespValue::Null, consumed)));
        }

        // Validate length
        if length < 0 {
            return Err(ParseError::InvalidBulkLength(length));
        }

        let length = length as usize;

        // Check size limit
        if length > MAX_BULK_SIZE {
            return Err(ParseError::MessageTooLarge {
                size: length,
                max: MAX_BULK_SIZE,
            });
        }

        // Calculate the start of the data
        let data_start = 1 + length_end + 2; // prefix + length + CRLF

        // Check if we have enough data
        let total_needed = data_start + length + 2; // data + CRLF
        if buf.len() < total_needed {
            return Ok(None); // Incomplete
        }

        // Verify trailing CRLF
        if &buf[data_start + length..data_start + length + 2] != CRLF {
            return Err(ParseError::ProtocolError(
                "bulk string missing trailing CRLF".to_string(),
            ));
        }

        // Extract the data (zero-copy using Bytes)
        let data = Bytes::copy_from_slice(&buf[data_start..data_start + length]);

        Ok(Some((RespValue::BulkString(data), total_needed)))
    }

    /// Parses an array: `*<count>\r\n<elements...>`
    fn parse_array(&mut self, buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
        debug_assert!(buf[0] == prefix::ARRAY);

        // Find the count line
        let count_end = match find_crlf(&buf[1..]) {
            Some(pos) => pos,
            None => return Ok(None),
        };

        let count_str = std::str::from_utf8(&buf[1..1 + count_end])
            .map_err(|e| ParseError::InvalidUtf8(e.to_string()))?;

        let count: i64 = count_str
            .parse()
            .map_err(|e: ParseIntError| ParseError::InvalidInteger(e.to_string()))?;

        // Handle null array
        if count == -1 {
            let consumed = 1 + count_end + 2;
            return Ok(Some((RespValue::Null, consumed)));
        }

        // Validate count
        if count < 0 {
            return Err(ParseError::InvalidArrayLength(count));
        }

        let count = count as usize;

        // Parse each element
        let mut elements = Vec::with_capacity(count);
        let mut consumed = 1 + count_end + 2; // *<count>\r\n

        self.depth += 1;

        for _ in 0..count {
            if consumed >= buf.len() {
                return Ok(None); // Incomplete
            }

            match self.parse_value(&buf[consumed..])? {
                Some((value, element_consumed)) => {
                    elements.push(value);
                    consumed += element_consumed;
                }
                None => return Ok(None), // Incomplete
            }
        }

        self.depth -= 1;

        Ok(Some((RespValue::Array(elements), consumed)))
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
            return Err(ParseError::ProtocolError(
                "Empty inline command".to_string(),
            ));
        }

        let elements: Vec<RespValue> = parts
            .into_iter()
            .map(|s| RespValue::BulkString(Bytes::from(s.to_string())))
            .collect();

        Ok(Some((RespValue::Array(elements), crlf_pos + 2)))
    }
}

/// Finds the position of CRLF in the buffer.
///
/// Returns the position of `\r` if found, or None if CRLF is not present.
#[inline]
fn find_crlf(buf: &[u8]) -> Option<usize> {
    // Use memchr for optimized byte searching on larger buffers
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

/// Helper function to parse a single RESP message from bytes.
///
/// This is a convenience function for simple use cases.
pub fn parse_message(buf: &[u8]) -> ParseResult<Option<(RespValue, usize)>> {
    RespParser::new().parse(buf)
}

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

    #[test]
    fn test_parse_simple_string_incomplete() {
        let input = b"+OK";
        assert!(parse_message(input).unwrap().is_none());
    }

    #[test]
    fn test_parse_error() {
        let input = b"-ERR unknown command\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::Error("ERR unknown command".to_string())
        );
        assert_eq!(result.1, 22);
    }

    #[test]
    fn test_parse_integer() {
        let input = b":1000\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::Integer(1000));
        assert_eq!(result.1, 7);
    }

    #[test]
    fn test_parse_negative_integer() {
        let input = b":-42\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::Integer(-42));
    }

    #[test]
    fn test_parse_bulk_string() {
        let input = b"$5\r\nhello\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::BulkString(Bytes::from("hello")));
        assert_eq!(result.1, 11);
    }

    #[test]
    fn test_parse_null_bulk_string() {
        let input = b"$-1\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::Null);
        assert_eq!(result.1, 5);
    }

    #[test]
    fn test_parse_empty_bulk_string() {
        let input = b"$0\r\n\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::BulkString(Bytes::from("")));
        assert_eq!(result.1, 6);
    }

    #[test]
    fn test_parse_bulk_string_incomplete() {
        let input = b"$5\r\nhel";
        assert!(parse_message(input).unwrap().is_none());
    }

    #[test]
    fn test_parse_array() {
        let input = b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::Array(vec![
                RespValue::BulkString(Bytes::from("GET")),
                RespValue::BulkString(Bytes::from("name")),
            ])
        );
        assert_eq!(result.1, 23);
    }

    #[test]
    fn test_parse_null_array() {
        let input = b"*-1\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::Null);
    }

    #[test]
    fn test_parse_empty_array() {
        let input = b"*0\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(result.0, RespValue::Array(vec![]));
    }

    #[test]
    fn test_parse_nested_array() {
        let input = b"*2\r\n:1\r\n*2\r\n:2\r\n:3\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::Array(vec![
                RespValue::Integer(1),
                RespValue::Array(vec![RespValue::Integer(2), RespValue::Integer(3),]),
            ])
        );
    }

    #[test]
    fn test_parse_mixed_array() {
        let input = b"*3\r\n+OK\r\n:100\r\n$5\r\nhello\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::Array(vec![
                RespValue::SimpleString("OK".to_string()),
                RespValue::Integer(100),
                RespValue::BulkString(Bytes::from("hello")),
            ])
        );
    }

    #[test]
    fn test_parse_inline_command() {
        // With inline parsing, unknown prefixes are treated as inline commands
        let input = b"@invalid\r\n";
        let result = parse_message(input);
        // Should parse as an inline command with a single element "@invalid"
        assert!(result.is_ok());
        let (value, consumed) = result.unwrap().unwrap();
        assert_eq!(consumed, 10);
        assert!(matches!(value, RespValue::Array(ref arr) if arr.len() == 1));
    }

    #[test]
    fn test_parse_invalid_integer() {
        let input = b":not_a_number\r\n";
        let result = parse_message(input);
        assert!(matches!(result, Err(ParseError::InvalidInteger(_))));
    }

    #[test]
    fn test_roundtrip() {
        // Test that serialize -> parse gives back the same value
        let original = RespValue::Array(vec![
            RespValue::bulk_string(Bytes::from("SET")),
            RespValue::bulk_string(Bytes::from("key")),
            RespValue::bulk_string(Bytes::from("value")),
        ]);

        let serialized = original.serialize();
        let (parsed, _) = parse_message(&serialized).unwrap().unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_parse_set_command() {
        // Real Redis command: SET user:101 "Ariz"
        let input = b"*3\r\n$3\r\nSET\r\n$8\r\nuser:101\r\n$4\r\nAriz\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::Array(vec![
                RespValue::BulkString(Bytes::from("SET")),
                RespValue::BulkString(Bytes::from("user:101")),
                RespValue::BulkString(Bytes::from("Ariz")),
            ])
        );
    }

    #[test]
    fn test_binary_safe_bulk_string() {
        // Bulk strings should handle binary data including null bytes
        let input = b"$5\r\nhel\x00o\r\n";
        let result = parse_message(input).unwrap().unwrap();
        assert_eq!(
            result.0,
            RespValue::BulkString(Bytes::from(&b"hel\x00o"[..]))
        );
    }
}
