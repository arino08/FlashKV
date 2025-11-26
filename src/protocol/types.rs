//! RESP (Redis Serialization Protocol) Data Types
//!
//! This module defines the data types used in the RESP protocol.
//! RESP is a simple, binary-safe protocol that Redis uses for client-server communication.
//!
//! ## Protocol Format
//!
//! Each RESP type starts with a type prefix byte:
//! - `+` Simple String
//! - `-` Error
//! - `:` Integer
//! - `$` Bulk String
//! - `*` Array
//!
//! All types are terminated with CRLF (`\r\n`).
//!
//! ## Examples
//!
//! Simple String: `+OK\r\n`
//! Error: `-ERR unknown command\r\n`
//! Integer: `:1000\r\n`
//! Bulk String: `$5\r\nhello\r\n`
//! Array: `*2\r\n$3\r\nGET\r\n$4\r\nname\r\n`
//! Null Bulk String: `$-1\r\n`

use bytes::Bytes;
use std::fmt;

/// The CRLF terminator used in RESP protocol
pub const CRLF: &[u8] = b"\r\n";

/// RESP protocol type prefixes
pub mod prefix {
    pub const SIMPLE_STRING: u8 = b'+';
    pub const ERROR: u8 = b'-';
    pub const INTEGER: u8 = b':';
    pub const BULK_STRING: u8 = b'$';
    pub const ARRAY: u8 = b'*';
}

/// Represents a value in the RESP protocol.
///
/// This enum covers all RESP data types and can be used for both
/// parsing incoming data and serializing outgoing responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    /// Simple strings are used for non-binary safe strings.
    /// They cannot contain CRLF characters.
    /// Format: `+<string>\r\n`
    SimpleString(String),

    /// Errors are similar to simple strings but indicate an error condition.
    /// Format: `-<error message>\r\n`
    Error(String),

    /// 64-bit signed integers.
    /// Format: `:<integer>\r\n`
    Integer(i64),

    /// Bulk strings are binary-safe strings up to 512 MB.
    /// Format: `$<length>\r\n<data>\r\n`
    /// Null bulk string: `$-1\r\n`
    BulkString(Bytes),

    /// Null value (null bulk string or null array)
    Null,

    /// Arrays can contain any RESP type, including nested arrays.
    /// Format: `*<count>\r\n<element1><element2>...`
    /// Null array: `*-1\r\n`
    Array(Vec<RespValue>),
}

impl RespValue {
    /// Creates a new simple string response.
    ///
    /// # Example
    /// ```
    /// use flashkv::protocol::types::RespValue;
    /// let ok = RespValue::simple_string("OK");
    /// ```
    pub fn simple_string(s: impl Into<String>) -> Self {
        RespValue::SimpleString(s.into())
    }

    /// Creates a new error response.
    ///
    /// # Example
    /// ```
    /// use flashkv::protocol::types::RespValue;
    /// let err = RespValue::error("ERR unknown command");
    /// ```
    pub fn error(s: impl Into<String>) -> Self {
        RespValue::Error(s.into())
    }

    /// Creates a new integer response.
    pub fn integer(n: i64) -> Self {
        RespValue::Integer(n)
    }

    /// Creates a new bulk string response.
    ///
    /// # Example
    /// ```
    /// use flashkv::protocol::types::RespValue;
    /// use bytes::Bytes;
    /// let bulk = RespValue::bulk_string(Bytes::from("hello"));
    /// ```
    pub fn bulk_string(data: impl Into<Bytes>) -> Self {
        RespValue::BulkString(data.into())
    }

    /// Creates a null response.
    pub fn null() -> Self {
        RespValue::Null
    }

    /// Creates an array response.
    pub fn array(values: Vec<RespValue>) -> Self {
        RespValue::Array(values)
    }

    /// Common response for successful operations
    pub fn ok() -> Self {
        RespValue::SimpleString("OK".to_string())
    }

    /// Common response for PONG
    pub fn pong() -> Self {
        RespValue::SimpleString("PONG".to_string())
    }

    /// Serializes the RESP value to bytes for sending over the wire.
    ///
    /// This method converts the RESP value into its wire format representation.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize_into(&mut buf);
        buf
    }

    /// Serializes the RESP value into an existing buffer.
    ///
    /// This is more efficient than `serialize()` when you want to reuse a buffer.
    pub fn serialize_into(&self, buf: &mut Vec<u8>) {
        match self {
            RespValue::SimpleString(s) => {
                buf.push(prefix::SIMPLE_STRING);
                buf.extend_from_slice(s.as_bytes());
                buf.extend_from_slice(CRLF);
            }
            RespValue::Error(s) => {
                buf.push(prefix::ERROR);
                buf.extend_from_slice(s.as_bytes());
                buf.extend_from_slice(CRLF);
            }
            RespValue::Integer(n) => {
                buf.push(prefix::INTEGER);
                buf.extend_from_slice(n.to_string().as_bytes());
                buf.extend_from_slice(CRLF);
            }
            RespValue::BulkString(data) => {
                buf.push(prefix::BULK_STRING);
                buf.extend_from_slice(data.len().to_string().as_bytes());
                buf.extend_from_slice(CRLF);
                buf.extend_from_slice(data);
                buf.extend_from_slice(CRLF);
            }
            RespValue::Null => {
                buf.push(prefix::BULK_STRING);
                buf.extend_from_slice(b"-1");
                buf.extend_from_slice(CRLF);
            }
            RespValue::Array(values) => {
                buf.push(prefix::ARRAY);
                buf.extend_from_slice(values.len().to_string().as_bytes());
                buf.extend_from_slice(CRLF);
                for value in values {
                    value.serialize_into(buf);
                }
            }
        }
    }

    /// Returns true if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, RespValue::Null)
    }

    /// Returns true if this value is an error.
    pub fn is_error(&self) -> bool {
        matches!(self, RespValue::Error(_))
    }

    /// Attempts to extract the inner string from SimpleString or BulkString.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RespValue::SimpleString(s) => Some(s),
            RespValue::BulkString(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

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

    /// Consumes self and returns the inner array if this is an Array variant.
    pub fn into_array(self) -> Option<Vec<RespValue>> {
        match self {
            RespValue::Array(arr) => Some(arr),
            _ => None,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_string_serialize() {
        let value = RespValue::simple_string("OK");
        assert_eq!(value.serialize(), b"+OK\r\n");
    }

    #[test]
    fn test_error_serialize() {
        let value = RespValue::error("ERR unknown command");
        assert_eq!(value.serialize(), b"-ERR unknown command\r\n");
    }

    #[test]
    fn test_integer_serialize() {
        let value = RespValue::integer(1000);
        assert_eq!(value.serialize(), b":1000\r\n");

        let negative = RespValue::integer(-42);
        assert_eq!(negative.serialize(), b":-42\r\n");
    }

    #[test]
    fn test_bulk_string_serialize() {
        let value = RespValue::bulk_string(Bytes::from("hello"));
        assert_eq!(value.serialize(), b"$5\r\nhello\r\n");
    }

    #[test]
    fn test_null_serialize() {
        let value = RespValue::null();
        assert_eq!(value.serialize(), b"$-1\r\n");
    }

    #[test]
    fn test_array_serialize() {
        let value = RespValue::array(vec![
            RespValue::bulk_string(Bytes::from("GET")),
            RespValue::bulk_string(Bytes::from("name")),
        ]);
        assert_eq!(value.serialize(), b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n");
    }

    #[test]
    fn test_nested_array_serialize() {
        let value = RespValue::array(vec![
            RespValue::integer(1),
            RespValue::array(vec![RespValue::integer(2), RespValue::integer(3)]),
        ]);
        assert_eq!(value.serialize(), b"*2\r\n:1\r\n*2\r\n:2\r\n:3\r\n");
    }

    #[test]
    fn test_ok_response() {
        assert_eq!(RespValue::ok().serialize(), b"+OK\r\n");
    }

    #[test]
    fn test_pong_response() {
        assert_eq!(RespValue::pong().serialize(), b"+PONG\r\n");
    }
}
