//! Write-Ahead Log (WAL) for batch persistence
//!
//! Each batch gets its own WAL file, allowing precise recovery of failed batches.
//!
//! WAL File Format:
//! - Location: `data/wal/grpc/{batch_id}.wal`
//! - Format: JSON (for debuggability)
//! - One file per batch
//!
//! Lifecycle:
//! 1. Write batch to WAL before processing
//! 2. Process batch (send to ingestion)
//! 3. Delete WAL on success
//! 4. Keep WAL on failure for replay

use anyhow::{Context, Result};
use openwit_ingestion::types::IngestedMessage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Batch processing status
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum BatchStatus {
    /// Batch written to WAL, pending processing
    Pending,
    /// Batch successfully processed
    Completed,
    /// Batch processing failed
    Failed,
}

/// WAL entry containing a complete batch
#[derive(Debug, Serialize, Deserialize)]
pub struct WalBatch {
    /// Unique batch identifier
    pub batch_id: String,
    /// Timestamp when batch was written to WAL
    pub wal_timestamp: u64,
    /// Number of messages in batch
    pub message_count: usize,
    /// Total size in bytes
    pub total_bytes: usize,
    /// Flush reason (size, timeout, shutdown)
    pub flush_reason: String,
    /// Processing status
    pub status: BatchStatus,
    /// Timestamp when batch processing completed (if applicable)
    pub completed_at: Option<u64>,
    /// All messages in the batch
    pub messages: Vec<IngestedMessage>,
}

/// WAL Manager handles batch persistence
pub struct WalManager {
    /// Base directory for WAL files
    wal_dir: PathBuf,
}

impl WalManager {
    /// Create a new WAL manager with the specified directory
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        let wal_dir = base_path.as_ref().join("wal").join("grpc");

        // Create WAL directory if it doesn't exist
        fs::create_dir_all(&wal_dir)
            .await
            .with_context(|| format!("Failed to create WAL directory: {:?}", wal_dir))?;

        tracing::info!("WAL manager initialized: {:?}", wal_dir);

        Ok(Self { wal_dir })
    }

    /// Write a batch to WAL before processing with index_id support
    /// Creates subdirectory structure: data/wal/grpc/{index_id}/{batch_id}.wal
    pub async fn write_batch_with_index(
        &self,
        batch_id: &str,
        messages: Vec<IngestedMessage>,
        total_bytes: usize,
        flush_reason: &str,
        index_id: &Option<String>,
    ) -> Result<()> {
        let wal_path = self.get_wal_path_with_index(batch_id, index_id);

        // Ensure the directory exists for this index_id
        if let Some(parent) = wal_path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create WAL subdirectory: {:?}", parent))?;
        }

        let wal_batch = WalBatch {
            batch_id: batch_id.to_string(),
            wal_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            message_count: messages.len(),
            total_bytes,
            flush_reason: flush_reason.to_string(),
            status: BatchStatus::Pending,
            completed_at: None,
            messages,
        };

        // Serialize to JSON
        let json_data = serde_json::to_vec_pretty(&wal_batch)
            .context("Failed to serialize batch to JSON")?;

        // Write atomically using a temp file
        let temp_path = wal_path.with_extension("wal.tmp");

        let mut file = fs::File::create(&temp_path)
            .await
            .with_context(|| format!("Failed to create temp WAL file: {:?}", temp_path))?;

        file.write_all(&json_data)
            .await
            .context("Failed to write WAL data")?;

        file.flush()
            .await
            .context("Failed to flush WAL data")?;

        // Ensure data is persisted to disk
        file.sync_all()
            .await
            .context("Failed to sync WAL file")?;

        // Atomic rename
        fs::rename(&temp_path, &wal_path)
            .await
            .with_context(|| format!("Failed to rename WAL file: {:?} -> {:?}", temp_path, wal_path))?;

        if let Some(ref idx) = index_id {
            tracing::debug!(
                "WAL written: batch_id={}, index_id={}, messages={}, size={} bytes, path={:?}",
                batch_id,
                idx,
                wal_batch.message_count,
                total_bytes,
                wal_path
            );
        } else {
            tracing::debug!(
                "WAL written: batch_id={} (default), messages={}, size={} bytes, path={:?}",
                batch_id,
                wal_batch.message_count,
                total_bytes,
                wal_path
            );
        }

        Ok(())
    }

    /// Write a batch to WAL before processing (backward compatibility)
    pub async fn write_batch(
        &self,
        batch_id: &str,
        messages: Vec<IngestedMessage>,
        total_bytes: usize,
        flush_reason: &str,
    ) -> Result<()> {
        self.write_batch_with_index(batch_id, messages, total_bytes, flush_reason, &None).await
    }

    /// Update batch status in WAL file with index_id support
    pub async fn update_batch_status_with_index(
        &self,
        batch_id: &str,
        status: BatchStatus,
        index_id: &Option<String>,
    ) -> Result<()> {
        let wal_path = self.get_wal_path_with_index(batch_id, index_id);

        // Read existing batch
        let mut wal_batch = self.read_batch_with_index(batch_id, index_id).await?;

        // Update status
        wal_batch.status = status;
        if matches!(wal_batch.status, BatchStatus::Completed | BatchStatus::Failed) {
            wal_batch.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }

        // Write back atomically
        let json_data = serde_json::to_vec_pretty(&wal_batch)
            .context("Failed to serialize batch to JSON")?;

        let temp_path = wal_path.with_extension("wal.tmp");

        let mut file = fs::File::create(&temp_path)
            .await
            .with_context(|| format!("Failed to create temp WAL file: {:?}", temp_path))?;

        file.write_all(&json_data)
            .await
            .context("Failed to write WAL data")?;

        file.flush()
            .await
            .context("Failed to flush WAL data")?;

        file.sync_all()
            .await
            .context("Failed to sync WAL file")?;

        fs::rename(&temp_path, &wal_path)
            .await
            .with_context(|| format!("Failed to rename WAL file: {:?} -> {:?}", temp_path, wal_path))?;

        if let Some(ref idx) = index_id {
            tracing::debug!("WAL status updated: batch_id={}, index_id={}, status={:?}", batch_id, idx, wal_batch.status);
        } else {
            tracing::debug!("WAL status updated: batch_id={} (default), status={:?}", batch_id, wal_batch.status);
        }

        Ok(())
    }

    /// Update batch status in WAL file (backward compatibility)
    pub async fn update_batch_status(&self, batch_id: &str, status: BatchStatus) -> Result<()> {
        self.update_batch_status_with_index(batch_id, status, &None).await
    }

    /// Delete WAL file with index_id support (only used by janitor)
    pub async fn delete_batch_with_index(&self, batch_id: &str, index_id: &Option<String>) -> Result<()> {
        let wal_path = self.get_wal_path_with_index(batch_id, index_id);

        if wal_path.exists() {
            fs::remove_file(&wal_path)
                .await
                .with_context(|| format!("Failed to delete WAL file: {:?}", wal_path))?;

            if let Some(ref idx) = index_id {
                tracing::debug!("WAL deleted: batch_id={}, index_id={}", batch_id, idx);
            } else {
                tracing::debug!("WAL deleted: batch_id={} (default)", batch_id);
            }
        }

        Ok(())
    }

    /// Delete WAL file (backward compatibility)
    pub async fn delete_batch(&self, batch_id: &str) -> Result<()> {
        self.delete_batch_with_index(batch_id, &None).await
    }

    /// Read a batch from WAL for replay with index_id support
    pub async fn read_batch_with_index(&self, batch_id: &str, index_id: &Option<String>) -> Result<WalBatch> {
        let wal_path = self.get_wal_path_with_index(batch_id, index_id);

        let json_data = fs::read(&wal_path)
            .await
            .with_context(|| format!("Failed to read WAL file: {:?}", wal_path))?;

        let wal_batch: WalBatch = serde_json::from_slice(&json_data)
            .context("Failed to deserialize WAL batch")?;

        if let Some(ref idx) = index_id {
            tracing::debug!("WAL read: batch_id={}, index_id={}, messages={}", batch_id, idx, wal_batch.message_count);
        } else {
            tracing::debug!("WAL read: batch_id={} (default), messages={}", batch_id, wal_batch.message_count);
        }

        Ok(wal_batch)
    }

    /// Read a batch from WAL for replay (backward compatibility)
    pub async fn read_batch(&self, batch_id: &str) -> Result<WalBatch> {
        self.read_batch_with_index(batch_id, &None).await
    }

    /// List all pending WAL files (batches that weren't processed successfully)
    pub async fn list_pending_batches(&self) -> Result<Vec<String>> {
        let mut batch_ids = Vec::new();

        let mut entries = fs::read_dir(&self.wal_dir)
            .await
            .with_context(|| format!("Failed to read WAL directory: {:?}", self.wal_dir))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                    batch_ids.push(file_stem.to_string());
                }
            }
        }

        tracing::info!("Found {} pending WAL batches", batch_ids.len());

        Ok(batch_ids)
    }

    /// Get the full path for a batch WAL file with index_id support
    /// Returns: data/wal/grpc/{index_id}/{batch_id}.wal or data/wal/grpc/{batch_id}.wal (default)
    fn get_wal_path_with_index(&self, batch_id: &str, index_id: &Option<String>) -> PathBuf {
        if let Some(idx) = index_id {
            self.wal_dir.join(idx).join(format!("{}.wal", batch_id))
        } else {
            self.wal_dir.join(format!("{}.wal", batch_id))
        }
    }

    /// Get the full path for a batch WAL file
    fn get_wal_path(&self, batch_id: &str) -> PathBuf {
        self.get_wal_path_with_index(batch_id, &None)
    }

    /// Get WAL directory path
    pub fn wal_directory(&self) -> &Path {
        &self.wal_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwit_ingestion::types::{MessagePayload, MessageSource};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_wal_write_and_read() {
        let temp_dir = TempDir::new().unwrap();
        let wal_manager = WalManager::new(temp_dir.path()).await.unwrap();

        let batch_id = "test-batch-123";
        let messages = vec![
            IngestedMessage {
                id: "msg-1".to_string(),
                received_at: 12345,
                size_bytes: 100,
                source: MessageSource::Grpc {
                    peer_addr: "127.0.0.1:50051".to_string(),
                },
                payload: MessagePayload::Trace(vec![1, 2, 3]),
                index_name: None,
            },
        ];

        // Write batch to WAL
        wal_manager
            .write_batch(batch_id, messages.clone(), 100, "test")
            .await
            .unwrap();

        // Read batch back
        let wal_batch = wal_manager.read_batch(batch_id).await.unwrap();

        assert_eq!(wal_batch.batch_id, batch_id);
        assert_eq!(wal_batch.message_count, 1);
        assert_eq!(wal_batch.messages.len(), 1);
        assert_eq!(wal_batch.messages[0].id, "msg-1");
    }

    #[tokio::test]
    async fn test_wal_delete() {
        let temp_dir = TempDir::new().unwrap();
        let wal_manager = WalManager::new(temp_dir.path()).await.unwrap();

        let batch_id = "test-batch-456";
        let messages = vec![
            IngestedMessage {
                id: "msg-1".to_string(),
                received_at: 12345,
                size_bytes: 100,
                source: MessageSource::Grpc {
                    peer_addr: "127.0.0.1:50051".to_string(),
                },
                payload: MessagePayload::Trace(vec![1, 2, 3]),
                index_name: None,
            },
        ];

        // Write batch
        wal_manager
            .write_batch(batch_id, messages, 100, "test")
            .await
            .unwrap();

        // Verify file exists
        let wal_path = wal_manager.get_wal_path(batch_id);
        assert!(wal_path.exists());

        // Delete batch
        wal_manager.delete_batch(batch_id).await.unwrap();

        // Verify file is gone
        assert!(!wal_path.exists());
    }

    #[tokio::test]
    async fn test_list_pending_batches() {
        let temp_dir = TempDir::new().unwrap();
        let wal_manager = WalManager::new(temp_dir.path()).await.unwrap();

        let messages = vec![
            IngestedMessage {
                id: "msg-1".to_string(),
                received_at: 12345,
                size_bytes: 100,
                source: MessageSource::Grpc {
                    peer_addr: "127.0.0.1:50051".to_string(),
                },
                payload: MessagePayload::Trace(vec![1, 2, 3]),
                index_name: None,
            },
        ];

        // Write multiple batches
        wal_manager.write_batch("batch-1", messages.clone(), 100, "test").await.unwrap();
        wal_manager.write_batch("batch-2", messages.clone(), 100, "test").await.unwrap();
        wal_manager.write_batch("batch-3", messages.clone(), 100, "test").await.unwrap();

        // List pending batches
        let pending = wal_manager.list_pending_batches().await.unwrap();

        assert_eq!(pending.len(), 3);
        assert!(pending.contains(&"batch-1".to_string()));
        assert!(pending.contains(&"batch-2".to_string()));
        assert!(pending.contains(&"batch-3".to_string()));
    }
}
