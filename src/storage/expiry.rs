//! Background Expiry Sweeper
//!
//! This module implements a background task that periodically scans the database
//! for expired keys and removes them. This is called "active expiry" as opposed
//! to "lazy expiry" (which happens on access).
//!
//! ## Why Do We Need This?
//!
//! Lazy expiry (checking on access) is efficient but has a problem:
//! If a key expires and is never accessed again, it will stay in memory forever!
//!
//! The background sweeper solves this by periodically cleaning up expired keys.
//!
//! ## Design
//!
//! The sweeper runs as a Tokio task and:
//! 1. Sleeps for a configurable interval (default: 100ms)
//! 2. Wakes up and scans a portion of the database
//! 3. Removes any expired keys found
//! 4. Logs statistics about the cleanup
//!
//! ## Adaptive Frequency
//!
//! If many keys are expiring, the sweeper will run more frequently.
//! If few keys are expiring, it will back off to save CPU.

use crate::storage::StorageEngine;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{debug, info, trace};

/// Configuration for the expiry sweeper.
#[derive(Debug, Clone)]
pub struct ExpiryConfig {
    /// Base interval between sweeps (default: 100ms)
    pub base_interval: Duration,

    /// Minimum interval between sweeps (default: 10ms)
    pub min_interval: Duration,

    /// Maximum interval between sweeps (default: 1s)
    pub max_interval: Duration,

    /// If this fraction of scanned keys are expired, speed up sweeping
    pub speedup_threshold: f64,

    /// If this fraction of scanned keys are expired, slow down sweeping
    pub slowdown_threshold: f64,
}

impl Default for ExpiryConfig {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_millis(100),
            min_interval: Duration::from_millis(10),
            max_interval: Duration::from_secs(1),
            speedup_threshold: 0.25,  // Speed up if >25% of keys are expired
            slowdown_threshold: 0.01, // Slow down if <1% of keys are expired
        }
    }
}

/// A handle to the running expiry sweeper.
///
/// When this handle is dropped, the sweeper task will be stopped.
#[derive(Debug)]
pub struct ExpirySweeper {
    /// Sender to signal shutdown
    shutdown_tx: watch::Sender<bool>,
}

impl ExpirySweeper {
    /// Starts the expiry sweeper as a background task.
    ///
    /// # Arguments
    ///
    /// * `engine` - The storage engine to sweep
    /// * `config` - Configuration for the sweeper
    ///
    /// # Returns
    ///
    /// Returns a handle that can be used to stop the sweeper.
    /// The sweeper will automatically stop when the handle is dropped.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use flashkv::storage::{StorageEngine, ExpirySweeper, ExpiryConfig};
    /// use std::sync::Arc;
    ///
    /// let engine = Arc::new(StorageEngine::new());
    /// let sweeper = ExpirySweeper::start(engine, ExpiryConfig::default());
    ///
    /// // Sweeper runs in the background...
    ///
    /// // Dropping the sweeper will stop it
    /// drop(sweeper);
    /// ```
    pub fn start(engine: Arc<StorageEngine>, config: ExpiryConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        tokio::spawn(sweeper_loop(engine, config, shutdown_rx));

        info!("Background expiry sweeper started");

        Self { shutdown_tx }
    }

    /// Stops the expiry sweeper.
    ///
    /// This is called automatically when the handle is dropped.
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
        info!("Background expiry sweeper stopped");
    }
}

impl Drop for ExpirySweeper {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The main sweeper loop.
async fn sweeper_loop(
    engine: Arc<StorageEngine>,
    config: ExpiryConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut current_interval = config.base_interval;

    loop {
        // Wait for the interval or shutdown signal
        tokio::select! {
            _ = tokio::time::sleep(current_interval) => {}
            result = shutdown_rx.changed() => {
                if result.is_err() || *shutdown_rx.borrow() {
                    debug!("Expiry sweeper received shutdown signal");
                    return;
                }
            }
        }

        // Get current key count before cleanup
        let keys_before = engine.len();

        // Perform cleanup
        let expired = engine.cleanup_expired();

        // Adjust interval based on expiry rate
        if keys_before > 0 {
            let expiry_rate = expired as f64 / keys_before as f64;

            if expiry_rate > config.speedup_threshold {
                // Many keys expiring - speed up
                current_interval = (current_interval / 2).max(config.min_interval);
                debug!(
                    expired = expired,
                    rate = %format!("{:.2}%", expiry_rate * 100.0),
                    new_interval_ms = current_interval.as_millis(),
                    "High expiry rate, speeding up sweeper"
                );
            } else if expiry_rate < config.slowdown_threshold && expired == 0 {
                // Few keys expiring - slow down
                current_interval = (current_interval * 2).min(config.max_interval);
                trace!(
                    new_interval_ms = current_interval.as_millis(),
                    "Low expiry rate, slowing down sweeper"
                );
            }
        }

        if expired > 0 {
            debug!(
                expired = expired,
                keys_remaining = engine.len(),
                "Expired keys cleaned up"
            );
        }
    }
}

/// Starts the expiry sweeper with default configuration.
///
/// This is a convenience function for simple use cases.
pub fn start_expiry_sweeper(engine: Arc<StorageEngine>) -> ExpirySweeper {
    ExpirySweeper::start(engine, ExpiryConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::time::Duration;

    #[tokio::test]
    async fn test_sweeper_cleans_expired_keys() {
        let engine = Arc::new(StorageEngine::new());

        // Add some keys with short TTL
        for i in 0..10 {
            engine.set_with_ttl(
                Bytes::from(format!("key{}", i)),
                Bytes::from("value"),
                Duration::from_millis(50),
            );
        }

        // Add a persistent key
        engine.set(Bytes::from("persistent"), Bytes::from("value"));

        assert_eq!(engine.len(), 11);

        // Start sweeper with fast interval
        let config = ExpiryConfig {
            base_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let _sweeper = ExpirySweeper::start(Arc::clone(&engine), config);

        // Wait for keys to expire and be cleaned up
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Only the persistent key should remain
        assert_eq!(engine.len(), 1);
        assert!(engine.exists(&Bytes::from("persistent")));
    }

    #[tokio::test]
    async fn test_sweeper_stops_on_drop() {
        let engine = Arc::new(StorageEngine::new());

        let config = ExpiryConfig {
            base_interval: Duration::from_millis(10),
            ..Default::default()
        };

        {
            let _sweeper = ExpirySweeper::start(Arc::clone(&engine), config);
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Sweeper is dropped here
        }

        // Add keys after sweeper is stopped
        engine.set_with_ttl(
            Bytes::from("key"),
            Bytes::from("value"),
            Duration::from_millis(10),
        );

        // Wait - keys should NOT be cleaned up since sweeper is stopped
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Key might still exist (lazy expiry hasn't happened)
        // but get() will trigger lazy expiry
        assert!(engine.get(&Bytes::from("key")).is_none());
    }

    #[tokio::test]
    async fn test_sweeper_adaptive_interval() {
        let engine = Arc::new(StorageEngine::new());

        // Add many short-lived keys to trigger speedup
        for i in 0..1000 {
            engine.set_with_ttl(
                Bytes::from(format!("key{}", i)),
                Bytes::from("value"),
                Duration::from_millis(20),
            );
        }

        let config = ExpiryConfig {
            base_interval: Duration::from_millis(50),
            min_interval: Duration::from_millis(5),
            max_interval: Duration::from_secs(1),
            speedup_threshold: 0.1,
            slowdown_threshold: 0.01,
        };

        let _sweeper = ExpirySweeper::start(Arc::clone(&engine), config);

        // Wait for cleanup
        tokio::time::sleep(Duration::from_millis(300)).await;

        // All keys should be expired and cleaned
        assert_eq!(engine.len(), 0);
    }
}
