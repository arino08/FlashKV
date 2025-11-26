//! FlashKV - A High-Performance In-Memory Key-Value Database
//!
//! This is the main entry point for the FlashKV server.
//! It sets up the TCP listener, storage engine, and handles incoming connections.

use flashkv::commands::CommandHandler;
use flashkv::connection::{handle_connection, ConnectionStats};
use flashkv::storage::{start_expiry_sweeper, StorageEngine};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

/// Server configuration
struct Config {
    /// Host to bind to
    host: String,
    /// Port to listen on
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6379,
        }
    }
}

impl Config {
    /// Parse configuration from command-line arguments
    fn from_args() -> Self {
        let mut config = Config::default();
        let args: Vec<String> = std::env::args().collect();

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--host" | "-h" => {
                    if i + 1 < args.len() {
                        config.host = args[i + 1].clone();
                        i += 2;
                    } else {
                        eprintln!("Error: --host requires a value");
                        std::process::exit(1);
                    }
                }
                "--port" | "-p" => {
                    if i + 1 < args.len() {
                        config.port = args[i + 1].parse().unwrap_or_else(|_| {
                            eprintln!("Error: invalid port number");
                            std::process::exit(1);
                        });
                        i += 2;
                    } else {
                        eprintln!("Error: --port requires a value");
                        std::process::exit(1);
                    }
                }
                "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "--version" | "-v" => {
                    println!("FlashKV version {}", flashkv::VERSION);
                    std::process::exit(0);
                }
                _ => {
                    eprintln!("Unknown argument: {}", args[i]);
                    print_help();
                    std::process::exit(1);
                }
            }
        }

        config
    }

    /// Returns the bind address as a string
    fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn print_help() {
    println!(
        r#"
FlashKV - A High-Performance In-Memory Key-Value Database

USAGE:
    flashkv [OPTIONS]

OPTIONS:
    -h, --host <HOST>    Host to bind to (default: 127.0.0.1)
    -p, --port <PORT>    Port to listen on (default: 6379)
    -v, --version        Print version information
        --help           Print this help message

EXAMPLES:
    flashkv                        # Start on 127.0.0.1:6379
    flashkv --port 6380            # Start on port 6380
    flashkv --host 0.0.0.0         # Listen on all interfaces

CONNECTING:
    Use redis-cli or any Redis client to connect:
    $ redis-cli -p 6379
    127.0.0.1:6379> PING
    PONG
    127.0.0.1:6379> SET name "Ariz"
    OK
    127.0.0.1:6379> GET name
    "Ariz"
"#
    );
}

fn print_banner(config: &Config) {
    println!(
        r#"
        
        ███████████ ████                    █████      █████   ████ █████   █████
       ░░███░░░░░░█░░███                   ░░███      ░░███   ███░ ░░███   ░░███ 
        ░███   █ ░  ░███   ██████    █████  ░███████   ░███  ███    ░███    ░███ 
        ░███████    ░███  ░░░░░███  ███░░   ░███░░███  ░███████     ░███    ░███ 
        ░███░░░█    ░███   ███████ ░░█████  ░███ ░███  ░███░░███    ░░███   ███  
        ░███  ░     ░███  ███░░███  ░░░░███ ░███ ░███  ░███ ░░███    ░░░█████░   
        █████       █████░░████████ ██████  ████ █████ █████ ░░████    ░░███     
       ░░░░░       ░░░░░  ░░░░░░░░ ░░░░░░  ░░░░ ░░░░░ ░░░░░   ░░░░      ░░░      
                                                                                 
       
FlashKV v{} - High-Performance In-Memory Key-Value Database
──────────────────────────────────────────────────────────────
Server started on {}
Ready to accept connections.

Use Ctrl+C to shutdown gracefully.
"#,
        flashkv::VERSION,
        config.bind_address()
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command-line arguments
    let config = Config::from_args();

    // Set up logging
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    // Print the banner
    print_banner(&config);

    // Create the storage engine (shared across all connections)
    let storage = Arc::new(StorageEngine::new());
    info!("Storage engine initialized with 64 shards");

    // Start the background expiry sweeper
    let _sweeper = start_expiry_sweeper(Arc::clone(&storage));
    info!("Background expiry sweeper started");

    // Create connection statistics
    let stats = Arc::new(ConnectionStats::new());

    // Bind the TCP listener
    let listener = TcpListener::bind(config.bind_address()).await?;
    info!("Listening on {}", config.bind_address());

    // Set up graceful shutdown
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("Shutdown signal received, stopping server...");
    };

    // Main accept loop
    tokio::select! {
        _ = accept_loop(listener, storage, stats) => {}
        _ = shutdown => {}
    }

    info!("Server shutdown complete");
    Ok(())
}

/// Main loop that accepts incoming connections
async fn accept_loop(
    listener: TcpListener,
    storage: Arc<StorageEngine>,
    stats: Arc<ConnectionStats>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                // Create a command handler for this connection
                let handler = CommandHandler::new(Arc::clone(&storage));
                let stats = Arc::clone(&stats);

                // Spawn a task to handle this connection
                tokio::spawn(async move {
                    handle_connection(stream, addr, handler, stats).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}
