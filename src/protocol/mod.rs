//! RESP Protocol Implementation
//!
//! This module provides a complete implementation of the Redis Serialization Protocol (RESP).
//!
//! ## Overview
//!
//! RESP is a simple, binary-safe protocol used by Redis for client-server communication.
//! It supports several data types and is designed to be easy to parse and serialize.
//!
//! ## Modules
//!
//! - `types`: Defines the `RespValue` enum and serialization
//! - `parser`: Zero-copy parser for incoming RESP data
//!
//! ## Example
//!
//! ```ignore
//! use flashkv::protocol::{RespValue, RespParser, parse_message};
//! use bytes::Bytes;
//!
//! // Parsing incoming data
//! let data = b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n";
//! let (value, consumed) = parse_message(data).unwrap().unwrap();
//!
//! // Creating responses
//! let response = RespValue::bulk_string(Bytes::from("Ariz"));
//! let bytes = response.serialize();
//! ```

pub mod parser;
pub mod types;

// Re-export commonly used types for convenience
pub use parser::{parse_message, ParseError, ParseResult, RespParser};
pub use types::RespValue;
