#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

// pub mod kafka; // Removed: Only proxy consumes from Kafka
// pub mod kafka_enhanced; // Removed: Only proxy consumes from Kafka
// pub mod dynamic_kafka; // Removed: Only proxy consumes from Kafka
// metrics removed - monitoring not required at this stage
pub mod types;
// buffer removed - using direct OTLP processing
// multi_threaded_buffer removed - using direct OTLP processing
// optimized_multi_threaded_buffer removed - unused experimental code
// WAL v2 removed - messages flow directly to storage
// batch_tracker and batch_status_logger removed - dead code from WAL era
// pipeline removed - uses buffer system
// pub mod multi_threaded_pipeline; // Removed: Using pipeline with v2
// inter_node_integration removed - dead code, not used in current architecture
// pub mod kafka_offset_manager; // Removed: Kafka offset management moved to openwit-kafka crate
// batch_router removed - dead code, replaced by DirectStorageRouter
// pub mod ingestion_server_v2; // Removed: Using regular ingestion_server with v2 WAL
// utils removed - dead code, only used by batch_router

// pub use kafka::{KafkaIngestor, KafkaMessage, KafkaConsumerPool, MessagePayload}; // Removed: Kafka consumption moved to openwit-kafka crate
// IngestionMetrics removed - monitoring not required at this stage
pub use types::{IngestedMessage, IngestionConfig, ControlCommand, MessageSource, MessagePayload as IngestPayload};
// pipeline exports removed - uses buffer system
// pub use multi_threaded_pipeline::MultiThreadedPipeline; // Removed: Using pipeline with v2
// buffer exports removed - using direct OTLP processing
// IngestionInterNodeIntegration removed - dead code, not used in current architecture
// pub use kafka_offset_manager::KafkaOffsetManager; // Removed: Kafka offset management moved to openwit-kafka crate
// BatchRouter removed - dead code, replaced by DirectStorageRouter

// ingestion_server removed - uses pipeline with buffer system

pub mod ingestion_receiver;
pub use ingestion_receiver::TelemetryIngestionGrpcService;

pub mod otlp_preserving_receiver;
pub use otlp_preserving_receiver::OtlpPreservingReceiver;

pub mod batched_otlp_receiver;
pub use batched_otlp_receiver::{BatchedOtlpReceiver, BatchConfig};

// Direct OTLP to Arrow converter - bypasses JSON entirely (12x faster!)
pub mod direct_otlp_to_arrow;
pub use direct_otlp_to_arrow::{DirectOtlpToArrowConverter, ConverterConfig, convert_otlp_batch_direct};

// Optimized ingestion receiver using direct conversion
pub mod optimized_ingestion_receiver;
pub use optimized_ingestion_receiver::OptimizedIngestionReceiver;

// Batch-preserving receiver that maintains Kafka batch integrity
pub mod batch_preserving_receiver;
pub use batch_preserving_receiver::{BatchPreservingIngestionReceiver, BatchProcessingConfig};
