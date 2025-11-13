//! Batching logic for OTLP data received via gRPC
//!
//! This module provides batching functionality with two flush criteria:
//! 1. Size-based: When batch reaches configured max size
//! 2. Time-based: When batch exists for longer than configured timeout
//!
//! When either criterion is met, the batch is sent to the flush worker pool
//! for parallel processing.

use anyhow::Result;
use openwit_ingestion::types::IngestedMessage;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use uuid::Uuid;

use crate::flush_pool::{FlushWorkerPool, ReadyBatch};

/// A single batch of ingested messages
#[derive(Debug)]
pub struct Batch {
    /// Unique batch identifier
    pub id: String,
    /// Messages accumulated in this batch
    pub messages: Vec<IngestedMessage>,
    /// When this batch was created
    pub created_at: Instant,
    /// When the first message was added to this batch (None if empty)
    pub first_message_at: Option<Instant>,
    /// Total size in bytes
    pub total_bytes: usize,
}

impl Batch {
    /// Create a new empty batch
    fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            messages: Vec::new(),
            created_at: Instant::now(),
            first_message_at: None,
            total_bytes: 0,
        }
    }

    /// Add a message to this batch
    fn add(&mut self, message: IngestedMessage) {
        // Set timestamp when first message is added
        if self.first_message_at.is_none() {
            self.first_message_at = Some(Instant::now());
        }
        self.total_bytes += message.size_bytes;
        self.messages.push(message);
    }

    /// Check if this batch is empty
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get the number of messages in this batch
    fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if this batch should be flushed based on size
    fn should_flush_by_size(&self, max_batch_size: usize) -> bool {
        self.messages.len() >= max_batch_size
    }

    /// Check if this batch should be flushed based on timeout
    /// Returns true if the batch has messages and the timeout since first message has elapsed
    fn should_flush_by_timeout(&self, timeout: Duration) -> bool {
        if let Some(first_msg_time) = self.first_message_at {
            first_msg_time.elapsed() >= timeout
        } else {
            // Empty batch, no timeout applies
            false
        }
    }
}

/// Configuration for the batcher
#[derive(Debug, Clone)]
pub struct BatcherConfig {
    /// Maximum number of messages per batch before flushing
    pub max_batch_size: usize,
    /// Maximum time to wait before flushing a batch
    pub batch_timeout: Duration,
}

impl Default for BatcherConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 5000,
            batch_timeout: Duration::from_secs(2),
        }
    }
}

impl BatcherConfig {
    /// Create configuration from IngestionConfig
    pub fn from_ingestion_config(config: &openwit_ingestion::types::IngestionConfig) -> Self {
        Self {
            max_batch_size: config.batch_size,
            batch_timeout: Duration::from_millis(config.batch_timeout_ms),
        }
    }
}

/// Error types for batcher operations
#[derive(Debug, thiserror::Error)]
pub enum BatcherError {
    #[error("Channel full - backpressure applied")]
    Backpressure,

    #[error("Batcher has been shutdown")]
    Shutdown,

    #[error("Failed to send message: {0}")]
    SendError(String),
}

/// Batcher manages batching of incoming OTLP messages using channels
pub struct Batcher {
    /// Channel for incoming messages (lock-free ingestion)
    ingress_tx: mpsc::Sender<IngestedMessage>,

    /// Configuration
    config: BatcherConfig,

    /// Handle to the batcher task
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Batcher {
    /// Create a new channel-based batcher with given configuration
    pub fn new(
        config: BatcherConfig,
        flush_pool: Arc<FlushWorkerPool>,
    ) -> Arc<Self> {
        // Channel capacity: 100k messages
        let channel_capacity = 100_000;
        let (ingress_tx, ingress_rx) = mpsc::channel(channel_capacity);

        tracing::info!(
            "Creating channel-based batcher: capacity={}, batch_size={}, timeout={:?}",
            channel_capacity,
            config.max_batch_size,
            config.batch_timeout
        );

        // Spawn the background batcher task
        let task_config = config.clone();
        let task_handle = tokio::spawn(async move {
            batcher_task(ingress_rx, task_config, flush_pool).await;
        });

        Arc::new(Self {
            ingress_tx,
            config,
            task_handle: Some(task_handle),
        })
    }

    /// Start the background flush task - now a no-op since batcher_task handles everything
    pub fn start_flush_task(self: Arc<Self>) {
        // The batcher_task already handles flushing, so this is a no-op
        // Kept for backward compatibility
    }

    /// Add a message to the batcher (non-blocking)
    pub fn try_add_message(&self, message: IngestedMessage) -> Result<(), BatcherError> {
        match self.ingress_tx.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("Batcher channel full - backpressure applied");
                Err(BatcherError::Backpressure)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("Batcher channel closed - shutdown in progress");
                Err(BatcherError::Shutdown)
            }
        }
    }

    /// Add a message to the current batch (blocking - for backward compatibility)
    pub async fn add_message(&self, message: IngestedMessage) -> Result<()> {
        self.ingress_tx
            .send(message)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send to batcher: {}", e))
    }

    /// Get the current channel depth (approximate)
    pub fn channel_depth(&self) -> usize {
        // Note: This is an approximation based on channel capacity
        // The exact depth is not directly available
        0 // Will be updated with proper tracking later
    }

    /// Force flush the current batch (useful for graceful shutdown)
    pub async fn force_flush(&self) -> Result<()> {
        // For now, just drop the sender which will cause batcher_task to flush
        // Proper shutdown will be implemented in Task 9
        Ok(())
    }
}

/// Background task that processes messages and manages batching
/// Now supports per-index-id batching
async fn batcher_task(
    mut ingress_rx: mpsc::Receiver<IngestedMessage>,
    config: BatcherConfig,
    flush_pool: Arc<FlushWorkerPool>,
) {
    use std::collections::HashMap;

    // Maintain separate batches per index_id
    let mut batches: HashMap<Option<String>, Batch> = HashMap::new();
    let mut timeout_check = interval(Duration::from_secs(1));

    tracing::info!("Batcher task started with per-index-id batching");

    loop {
        tokio::select! {
            // Receive message from ingress channel
            Some(msg) = ingress_rx.recv() => {
                let index_id = msg.index_name.clone();

                // Get or create batch for this index_id
                let current_batch = batches.entry(index_id.clone()).or_insert_with(Batch::new);

                // Set first message timestamp if batch is empty
                if current_batch.is_empty() {
                    current_batch.first_message_at = Some(Instant::now());
                }

                // Add message to batch
                current_batch.add(msg);

                if let Some(ref idx) = index_id {
                    tracing::debug!(
                        "Added message to batch for index_id={}, current size: {}/{}",
                        idx,
                        current_batch.len(),
                        config.max_batch_size
                    );
                } else {
                    tracing::debug!(
                        "Added message to default batch, current size: {}/{}",
                        current_batch.len(),
                        config.max_batch_size
                    );
                }

                // Check if batch is full (size-based flush)
                if current_batch.should_flush_by_size(config.max_batch_size) {
                    flush_batch_with_index(current_batch, &flush_pool, "size", &index_id).await;
                    batches.remove(&index_id);
                }
            }

            // Check for timeout-based flush
            _ = timeout_check.tick() => {
                // Check all batches for timeout
                let mut to_flush = Vec::new();

                for (index_id, batch) in batches.iter() {
                    if batch.should_flush_by_timeout(config.batch_timeout) {
                        to_flush.push(index_id.clone());
                    }
                }

                // Flush timed-out batches
                for index_id in to_flush {
                    if let Some(mut batch) = batches.remove(&index_id) {
                        flush_batch_with_index(&mut batch, &flush_pool, "timeout", &index_id).await;
                    }
                }
            }

            // Channel closed - shutdown
            else => {
                tracing::info!("Batcher task shutting down...");

                // Flush all remaining batches
                for (index_id, mut batch) in batches.drain() {
                    if !batch.is_empty() {
                        flush_batch_with_index(&mut batch, &flush_pool, "shutdown", &index_id).await;
                    }
                }

                break;
            }
        }
    }

    tracing::info!("Batcher task terminated");
}

/// Helper function to flush a batch with index_id information
async fn flush_batch_with_index(
    current_batch: &mut Batch,
    flush_pool: &Arc<FlushWorkerPool>,
    reason: &str,
    index_id: &Option<String>,
) {
    if current_batch.is_empty() {
        return;
    }

    let batch_id = current_batch.id.clone();
    let message_count = current_batch.len();
    let total_bytes = current_batch.total_bytes;

    // Calculate age since first message
    let age_since_first_message = current_batch
        .first_message_at
        .map(|t| t.elapsed())
        .unwrap_or_else(|| current_batch.created_at.elapsed());

    // Print batch info to console with index_id
    if let Some(ref idx) = index_id {
        println!(
            "Flushing batch [{}] for index_id={} - Reason: {} | Messages: {} | Size: {} bytes | Age: {:?}",
            batch_id,
            idx,
            reason,
            message_count,
            total_bytes,
            age_since_first_message
        );
    } else {
        println!(
            "Flushing batch [{}] (default) - Reason: {} | Messages: {} | Size: {} bytes | Age: {:?}",
            batch_id,
            reason,
            message_count,
            total_bytes,
            age_since_first_message
        );
    }

    // Create ReadyBatch for worker pool with index_id
    let ready_batch = ReadyBatch {
        id: batch_id.clone(),
        messages: std::mem::take(&mut current_batch.messages),
        total_bytes,
        created_at: current_batch.created_at,
        reason: reason.to_string(),
        index_id: index_id.clone(),
    };

    // Submit to worker pool (non-blocking)
    if let Err(e) = flush_pool.submit_batch(ready_batch).await {
        tracing::error!(
            "Failed to submit batch {} to worker pool: {}",
            batch_id, e
        );
    } else {
        if let Some(ref idx) = index_id {
            tracing::info!(
                batch_id = %batch_id,
                index_id = %idx,
                reason = %reason,
                message_count = message_count,
                total_bytes = total_bytes,
                "Batch submitted to worker pool with index_id"
            );
        } else {
            tracing::info!(
                batch_id = %batch_id,
                reason = %reason,
                message_count = message_count,
                total_bytes = total_bytes,
                "Batch submitted to worker pool (default)"
            );
        }
    }

    // Create new empty batch
    *current_batch = Batch::new();
}

/// Helper function to flush a batch (backward compatibility - calls flush_batch_with_index)
async fn flush_batch(
    current_batch: &mut Batch,
    flush_pool: &Arc<FlushWorkerPool>,
    reason: &str,
) {
    flush_batch_with_index(current_batch, flush_pool, reason, &None).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwit_ingestion::types::{MessagePayload, MessageSource};

    #[test]
    fn test_batch_creation() {
        let batch = Batch::new();
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.total_bytes, 0);
    }

    #[test]
    fn test_batch_add_message() {
        let mut batch = Batch::new();
        let message = IngestedMessage {
            id: "test-1".to_string(),
            received_at: 12345,
            size_bytes: 100,
            source: MessageSource::Grpc {
                peer_addr: "127.0.0.1:50052".to_string(),
            },
            payload: MessagePayload::Trace(vec![1, 2, 3]),
            index_name: None,
        };

        batch.add(message);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.total_bytes, 100);
    }

    #[test]
    fn test_batch_should_flush_by_size() {
        let mut batch = Batch::new();
        assert!(!batch.should_flush_by_size(10));

        for i in 0..10 {
            let message = IngestedMessage {
                id: format!("test-{}", i),
                received_at: 12345,
                size_bytes: 10,
                source: MessageSource::Grpc {
                    peer_addr: "127.0.0.1:50052".to_string(),
                },
                payload: MessagePayload::Trace(vec![1, 2, 3]),
                index_name: None,
            };
            batch.add(message);
        }

        assert!(batch.should_flush_by_size(10));
        assert!(!batch.should_flush_by_size(20));
    }

    #[test]
    fn test_batch_should_flush_by_timeout() {
        let batch = Batch::new();
        assert!(!batch.should_flush_by_timeout(Duration::from_secs(10)));

        std::thread::sleep(Duration::from_millis(100));
        assert!(batch.should_flush_by_timeout(Duration::from_millis(50)));
    }
}
