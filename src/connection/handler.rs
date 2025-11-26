//! Connection Handler Module
//!
//! This module handles individual client connections to FlashKV.
//! Each client gets its own handler task that runs in a loop,
//! reading commands and sending responses.
//!
//! ## Connection Lifecycle
//!
//! ```text
//! 1. Client connects (TCP handshake)
//!        │
//!        ▼
//! 2. ConnectionHandler spawned
//!        │
//!        ▼
//! 3. ┌──────────────────────────────┐
//!    │      Main Loop               │
//!    │                              │
//!    │  ┌─────────────────────────┐ │
//!    │  │ Read bytes from socket  │ │
//!    │  └───────────┬─────────────┘ │
//!    │              │               │
//!    │              ▼               │
//!    │  ┌─────────────────────────┐ │
//!    │  │ Parse RESP command      │ │
//!    │  └───────────┬─────────────┘ │
//!    │              │               │
//!    │              ▼               │
//!    │  ┌─────────────────────────┐ │
//!    │  │ Execute command         │ │
//!    │  └───────────┬─────────────┘ │
//!    │              │               │
//!    │              ▼               │
//!    │  ┌─────────────────────────┐ │
//!    │  │ Send response           │ │
//!    │  └───────────┬─────────────┘ │
//!    │              │               │
//!    │              ▼               │
//!    │         [Loop back]          │
//!    └──────────────────────────────┘
//!        │
//!        ▼
//! 4. Client disconnects / error
//!        │
//!        ▼
//! 5. Handler task ends
//! ```
//!
//! ## Buffer Management
//!
//! We use a BytesMut buffer to accumulate incoming data. This is important
//! because TCP is a stream protocol - we might receive partial commands,
//! or multiple commands in a single read.

use crate::commands::CommandHandler;
use crate::protocol::{ParseError, RespParser, RespValue};
use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpStream;
use tracing::{debug, error, info, trace, warn};

/// Maximum size for the read buffer (64 KB)
const MAX_BUFFER_SIZE: usize = 64 * 1024;

/// Initial buffer capacity
const INITIAL_BUFFER_SIZE: usize = 4096;

/// Statistics for connection handling
#[derive(Debug, Default)]
pub struct ConnectionStats {
    /// Total number of connections accepted
    pub connections_accepted: AtomicU64,
    /// Currently active connections
    pub active_connections: AtomicU64,
    /// Total commands processed
    pub commands_processed: AtomicU64,
    /// Total bytes read
    pub bytes_read: AtomicU64,
    /// Total bytes written
    pub bytes_written: AtomicU64,
}

impl ConnectionStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connection_opened(&self) {
        self.connections_accepted.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn command_processed(&self) {
        self.commands_processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn bytes_read(&self, count: usize) {
        self.bytes_read.fetch_add(count as u64, Ordering::Relaxed);
    }

    pub fn bytes_written(&self, count: usize) {
        self.bytes_written
            .fetch_add(count as u64, Ordering::Relaxed);
    }
}

/// Handles a single client connection.
///
/// This struct manages the read buffer, parsing, and response sending
/// for one connected client.
pub struct ConnectionHandler {
    /// The TCP stream for this connection
    stream: BufWriter<TcpStream>,

    /// Client's address (for logging)
    addr: SocketAddr,

    /// Buffer for incoming data
    buffer: BytesMut,

    /// The command handler (shared across connections)
    command_handler: CommandHandler,

    /// RESP parser
    parser: RespParser,

    /// Connection statistics (shared)
    stats: Arc<ConnectionStats>,
}

impl ConnectionHandler {
    /// Creates a new connection handler.
    ///
    /// # Arguments
    ///
    /// * `stream` - The TCP stream for this connection
    /// * `addr` - The client's socket address
    /// * `command_handler` - The command handler for executing commands
    /// * `stats` - Shared connection statistics
    pub fn new(
        stream: TcpStream,
        addr: SocketAddr,
        command_handler: CommandHandler,
        stats: Arc<ConnectionStats>,
    ) -> Self {
        stats.connection_opened();

        Self {
            stream: BufWriter::new(stream),
            addr,
            buffer: BytesMut::with_capacity(INITIAL_BUFFER_SIZE),
            command_handler,
            parser: RespParser::new(),
            stats,
        }
    }

    /// Runs the main connection loop.
    ///
    /// This method reads commands from the client, executes them,
    /// and sends back responses until the client disconnects or an error occurs.
    pub async fn run(mut self) -> Result<(), ConnectionError> {
        info!(client = %self.addr, "Client connected");

        let result = self.main_loop().await;

        match &result {
            Ok(()) => info!(client = %self.addr, "Client disconnected gracefully"),
            Err(e) => match e {
                ConnectionError::ClientDisconnected => {
                    debug!(client = %self.addr, "Client disconnected")
                }
                ConnectionError::IoError(io_err)
                    if io_err.kind() == std::io::ErrorKind::ConnectionReset =>
                {
                    debug!(client = %self.addr, "Connection reset by client")
                }
                _ => warn!(client = %self.addr, error = %e, "Connection error"),
            },
        }

        self.stats.connection_closed();
        result
    }

    /// The main read-execute-respond loop.
    async fn main_loop(&mut self) -> Result<(), ConnectionError> {
        loop {
            // Try to parse a complete command from the buffer
            while let Some(command) = self.try_parse_command()? {
                // Execute the command
                let response = self.command_handler.execute(command);
                self.stats.command_processed();

                // Check for QUIT command
                if matches!(&response, RespValue::SimpleString(s) if s == "OK") {
                    // Could be QUIT, but we'll just send response and continue
                }

                // Send the response
                self.send_response(&response).await?;
            }

            // Need more data - read from the socket
            self.read_more_data().await?;
        }
    }

    /// Attempts to parse a command from the buffer.
    fn try_parse_command(&mut self) -> Result<Option<RespValue>, ConnectionError> {
        if self.buffer.is_empty() {
            return Ok(None);
        }

        match self.parser.parse(&self.buffer) {
            Ok(Some((value, consumed))) => {
                // Successfully parsed a command - consume the bytes
                let _ = self.buffer.split_to(consumed);
                trace!(
                    client = %self.addr,
                    consumed = consumed,
                    remaining = self.buffer.len(),
                    "Parsed command"
                );
                Ok(Some(value))
            }
            Ok(None) => {
                // Incomplete data - need to read more
                trace!(
                    client = %self.addr,
                    buffered = self.buffer.len(),
                    "Incomplete command, need more data"
                );
                Ok(None)
            }
            Err(e) => {
                // Parse error - send error response and clear buffer
                warn!(client = %self.addr, error = %e, "Parse error");
                Err(ConnectionError::ParseError(e))
            }
        }
    }

    /// Reads more data from the socket into the buffer.
    async fn read_more_data(&mut self) -> Result<(), ConnectionError> {
        // Check buffer size limit
        if self.buffer.len() >= MAX_BUFFER_SIZE {
            error!(
                client = %self.addr,
                size = self.buffer.len(),
                "Buffer size limit exceeded"
            );
            return Err(ConnectionError::BufferFull);
        }

        // Ensure we have some capacity
        if self.buffer.capacity() - self.buffer.len() < 1024 {
            self.buffer.reserve(4096);
        }

        // Read data
        let n = self.stream.get_mut().read_buf(&mut self.buffer).await?;

        if n == 0 {
            // Connection closed by client
            if self.buffer.is_empty() {
                return Err(ConnectionError::ClientDisconnected);
            } else {
                // Partial command in buffer
                return Err(ConnectionError::UnexpectedEof);
            }
        }

        self.stats.bytes_read(n);
        trace!(client = %self.addr, bytes = n, "Read data");

        Ok(())
    }

    /// Sends a response to the client.
    async fn send_response(&mut self, response: &RespValue) -> Result<(), ConnectionError> {
        let bytes = response.serialize();
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;
        self.stats.bytes_written(bytes.len());
        trace!(
            client = %self.addr,
            bytes = bytes.len(),
            "Sent response"
        );
        Ok(())
    }
}

/// Errors that can occur while handling a connection.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// I/O error (network issue)
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// RESP parse error
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),

    /// Client disconnected normally
    #[error("Client disconnected")]
    ClientDisconnected,

    /// Unexpected end of stream (partial command)
    #[error("Unexpected end of stream")]
    UnexpectedEof,

    /// Buffer size limit exceeded
    #[error("Buffer size limit exceeded")]
    BufferFull,
}

/// Handles a client connection.
///
/// This is a convenience function that creates a ConnectionHandler
/// and runs it to completion.
///
/// # Arguments
///
/// * `stream` - The TCP stream for this connection
/// * `addr` - The client's socket address
/// * `command_handler` - The command handler for executing commands
/// * `stats` - Shared connection statistics
pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    command_handler: CommandHandler,
    stats: Arc<ConnectionStats>,
) {
    let handler = ConnectionHandler::new(stream, addr, command_handler, stats);
    if let Err(e) = handler.run().await {
        match e {
            ConnectionError::ClientDisconnected => {}
            ConnectionError::IoError(ref io_err)
                if io_err.kind() == std::io::ErrorKind::ConnectionReset => {}
            _ => {
                debug!(client = %addr, error = %e, "Connection ended with error");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageEngine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn create_test_server() -> (SocketAddr, Arc<StorageEngine>, Arc<ConnectionStats>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let storage = Arc::new(StorageEngine::new());
        let stats = Arc::new(ConnectionStats::new());

        let storage_clone = Arc::clone(&storage);
        let stats_clone = Arc::clone(&stats);

        tokio::spawn(async move {
            while let Ok((stream, client_addr)) = listener.accept().await {
                let handler = CommandHandler::new(Arc::clone(&storage_clone));
                let stats = Arc::clone(&stats_clone);
                tokio::spawn(handle_connection(stream, client_addr, handler, stats));
            }
        });

        (addr, storage, stats)
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let (addr, _, _) = create_test_server().await;

        let mut client = TcpStream::connect(addr).await.unwrap();

        // Send PING command
        client.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();

        // Read response
        let mut buf = [0u8; 64];
        let n = client.read(&mut buf).await.unwrap();

        assert_eq!(&buf[..n], b"+PONG\r\n");
    }

    #[tokio::test]
    async fn test_set_get() {
        let (addr, _, _) = create_test_server().await;

        let mut client = TcpStream::connect(addr).await.unwrap();

        // Send SET command
        client
            .write_all(b"*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$4\r\nAriz\r\n")
            .await
            .unwrap();

        let mut buf = [0u8; 64];
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"+OK\r\n");

        // Send GET command
        client
            .write_all(b"*2\r\n$3\r\nGET\r\n$4\r\nname\r\n")
            .await
            .unwrap();

        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"$4\r\nAriz\r\n");
    }

    #[tokio::test]
    async fn test_multiple_commands() {
        let (addr, _, _) = create_test_server().await;

        let mut client = TcpStream::connect(addr).await.unwrap();

        // Send multiple commands in one write (pipelining)
        // Note: Each command must be properly formatted without extra whitespace
        client
            .write_all(b"*3\r\n$3\r\nSET\r\n$2\r\nk1\r\n$2\r\nv1\r\n*3\r\n$3\r\nSET\r\n$2\r\nk2\r\n$2\r\nv2\r\n*2\r\n$3\r\nGET\r\n$2\r\nk1\r\n*2\r\n$3\r\nGET\r\n$2\r\nk2\r\n")
            .await
            .unwrap();

        let mut buf = vec![0u8; 256];
        let mut total = 0;

        // Read all responses with a timeout
        let timeout = tokio::time::Duration::from_secs(2);
        let deadline = tokio::time::Instant::now() + timeout;

        // Expected: +OK\r\n+OK\r\n$2\r\nv1\r\n$2\r\nv2\r\n (30 bytes)
        while total < 30 && tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                client.read(&mut buf[total..]),
            )
            .await
            {
                Ok(Ok(n)) if n > 0 => total += n,
                _ => break,
            }
        }

        let response = String::from_utf8_lossy(&buf[..total]);
        assert!(response.contains("+OK"));
        assert!(response.contains("v1"));
        assert!(response.contains("v2"));
    }

    #[tokio::test]
    async fn test_connection_stats() {
        let (addr, _, stats) = create_test_server().await;

        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 0);

        let mut client = TcpStream::connect(addr).await.unwrap();

        // Give the server time to accept the connection
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(stats.connections_accepted.load(Ordering::Relaxed), 1);
        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 1);

        // Send a command
        client.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
        let mut buf = [0u8; 64];
        let _ = client.read(&mut buf).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert!(stats.commands_processed.load(Ordering::Relaxed) >= 1);
        assert!(stats.bytes_read.load(Ordering::Relaxed) > 0);
        assert!(stats.bytes_written.load(Ordering::Relaxed) > 0);

        // Close connection
        drop(client);

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(stats.active_connections.load(Ordering::Relaxed), 0);
    }
}
