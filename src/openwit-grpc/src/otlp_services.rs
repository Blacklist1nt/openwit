use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use std::sync::Arc;

use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{
            logs_service_server::LogsService as OtelLogsServiceTrait,
            ExportLogsServiceRequest, ExportLogsServiceResponse,
        },
        metrics::v1::{
            metrics_service_server::MetricsService as OtelMetricsServiceTrait,
            ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        },
        trace::v1::{
            trace_service_server::TraceService as OtelTraceServiceTrait,
            ExportTraceServiceRequest, ExportTraceServiceResponse,
        },
    },
    trace::v1 as OtlpTraceProtoTypes,
    metrics::v1 as OtlpMetricsProtoTypes,
    logs::v1 as OtlpLogsProtoTypes,
};

use opentelemetry_proto::tonic::metrics::v1::metric::Data;

// use openwit_metrics::{
//     EXPORT_LATENCY, INGEST_LOG_COUNTER, INGEST_METRIC_DP_COUNTER, INGEST_SPAN_COUNTER,
// };

// Import our formatting helpers and batcher
use crate::format_helpers;
use crate::batcher::Batcher;
type TelemetryOutputSender = mpsc::Sender<String>;

// --- Trace Service Implementation ---
#[derive(Clone)]
pub struct OtlpTraceService {
    output_tx: TelemetryOutputSender,
    batcher: Option<Arc<Batcher>>,
}

impl OtlpTraceService {
    pub fn new(output_tx: TelemetryOutputSender) -> Self {
        Self { output_tx, batcher: None }
    }

    pub fn with_batcher(mut self, batcher: Arc<Batcher>) -> Self {
        self.batcher = Some(batcher);
        self
    }
}

#[allow(deprecated)]
#[tonic::async_trait]
impl OtelTraceServiceTrait for OtlpTraceService {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        let remote_addr = request.remote_addr().map_or_else(
            || "unknown".to_string(),
            |addr| addr.to_string(),
        );

        // Extract index_id from gRPC metadata (headers)
        let index_id = request.metadata().get("index-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(ref idx) = index_id {
            tracing::info!(peer.addr = %remote_addr, index_id = %idx, "Received ExportTraceServiceRequest with index_id");
        } else {
            tracing::info!(peer.addr = %remote_addr, "Received ExportTraceServiceRequest");
        }

        // let _timer = EXPORT_LATENCY.with_label_values(&["logs"]).start_timer();

        // --- existing counting logic ......................
        let span_count = request
            .get_ref()
            .resource_spans
            .iter()
            .flat_map(|rs| &rs.scope_spans)
            .flat_map(|ss| &ss.spans)
            .count();

        // INGEST_SPAN_COUNTER.inc_by(span_count as u64);

        let req_data = request.into_inner();

        // If we have a batcher, add traces to batch for storage with index_id
        if let Some(ref batcher) = self.batcher {
            match crate::otlp_ingest_connector::process_otlp_traces_batched_with_index(
                req_data.clone(),
                batcher,
                index_id.clone(),
            ).await {
                Ok(count) => {
                    if let Some(ref idx) = index_id {
                        tracing::info!(index_id = %idx, "Added {} traces to batch with index_id", count);
                    } else {
                        tracing::info!("Added {} traces to batch", count);
                    }
                },
                Err(e) => tracing::error!("Failed to add traces to batch: {}", e),
            }
        }
        
        // Create one-line messages for each span
        for resource_span_batch in req_data.resource_spans {
            let resource_attrs = if let Some(resource) = &resource_span_batch.resource {
                resource.attributes.iter()
                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };

            for scope_spans in resource_span_batch.scope_spans {

                for span in scope_spans.spans {
                    let trace_id_hex = span.trace_id.iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();
                    let span_id_hex = span.span_id.iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();
                    
                    let span_kind = match OtlpTraceProtoTypes::span::SpanKind::from_i32(span.kind) {
                        Some(k) => format!("{:?}", k),
                        None => "Unknown".to_string()
                    };
                    
                    let status = if let Some(s) = &span.status {
                        format!(" status={:?}", OtlpTraceProtoTypes::status::StatusCode::from_i32(s.code).unwrap_or(OtlpTraceProtoTypes::status::StatusCode::Unset))
                    } else {
                        String::new()
                    };
                    
                    let attrs = span.attributes.iter()
                        .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                        .collect::<Vec<_>>()
                        .join(",");
                    
                    let duration_ms = (span.end_time_unix_nano - span.start_time_unix_nano) as f64 / 1_000_000.0;
                    
                    let one_line_message = format!(
                        "TRACE [{} {}] name={} trace_id={} span_id={} kind={} duration_ms={:.3}{} resource=[{}] attrs=[{}]",
                        remote_addr,
                        format_helpers::format_timestamp_ns(span.start_time_unix_nano),
                        span.name,
                        trace_id_hex,
                        span_id_hex,
                        span_kind,
                        duration_ms,
                        status,
                        resource_attrs,
                        attrs
                    );
                    
                    // Send each span as a separate message
                    if self.output_tx.send(one_line_message).await.is_err() {
                        tracing::error!("Failed to send formatted trace data to output channel");
                    }
                    
                    // Add debug logging
                    tracing::debug!("Successfully ingested trace - TraceID: {} (gRPC endpoint working correctly)", trace_id_hex);
                }
            }
        }
        
        // Log ingestion stats similar to HTTP endpoint
        tracing::info!("Trace ingestion stats via gRPC - Received {} spans from {}", span_count, remote_addr);

        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

// --- Metrics Service Implementation ---
// src/otlp_services.rs
// ... (imports and OtlpTraceService are above this) ...

// --- Metrics Service Implementation ---
#[derive(Clone)]
pub struct OtlpMetricsService {
    output_tx: TelemetryOutputSender,
    batcher: Option<Arc<Batcher>>,
}

impl OtlpMetricsService {
    pub fn new(output_tx: TelemetryOutputSender) -> Self {
        Self { output_tx, batcher: None }
    }

    pub fn with_batcher(mut self, batcher: Arc<Batcher>) -> Self {
        self.batcher = Some(batcher);
        self
    }
}

#[tonic::async_trait]
impl OtelMetricsServiceTrait for OtlpMetricsService {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>, // <--- Ensure this parameter is here
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let remote_addr = request.remote_addr().map_or_else(
            || "unknown".to_string(),
            |addr| addr.to_string(),
        );

        // Extract index_id from gRPC metadata (headers)
        let index_id = request.metadata().get("index-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(ref idx) = index_id {
            tracing::info!(peer.addr = %remote_addr, index_id = %idx, "Received ExportMetricsServiceRequest with index_id");
        } else {
            tracing::info!(peer.addr = %remote_addr, "Received ExportMetricsServiceRequest");
        }

        let metric_dp_count = request
            .get_ref()
            .resource_metrics
            .iter()
            .flat_map(|rm| &rm.scope_metrics)
            .flat_map(|sm| &sm.metrics)
            .map(|m| match &m.data {
                Some(Data::Gauge(g))      => g.data_points.len(),
                Some(Data::Sum(s))        => s.data_points.len(),
                Some(Data::Histogram(h))  => h.data_points.len(),
                _ => 0,
            })
            .sum::<usize>();

        // INGEST_METRIC_DP_COUNTER.inc_by(metric_dp_count as u64);

        let req_data = request.into_inner();

        // If we have a batcher, add metrics to batch for storage with index_id
        if let Some(ref batcher) = self.batcher {
            match crate::otlp_ingest_connector::process_otlp_metrics_batched_with_index(
                req_data.clone(),
                batcher,
                index_id.clone(),
            ).await {
                Ok(count) => {
                    if let Some(ref idx) = index_id {
                        tracing::info!(index_id = %idx, "Added {} metrics to batch with index_id", count);
                    } else {
                        tracing::info!("Added {} metrics to batch", count);
                    }
                },
                Err(e) => tracing::error!("Failed to add metrics to batch: {}", e),
            }
        }
        
        // Create one-line messages for each metric data point
        for resource_metrics_batch in req_data.resource_metrics {
            let resource_attrs = if let Some(resource) = &resource_metrics_batch.resource {
                resource.attributes.iter()
                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };

            for scope_metrics in resource_metrics_batch.scope_metrics {

                for metric in scope_metrics.metrics {
                    match &metric.data {
                        Some(OtlpMetricsProtoTypes::metric::Data::Gauge(gauge)) => {
                            for dp in &gauge.data_points {
                                let value = match &dp.value {
                                    Some(v) => match v {
                                        OtlpMetricsProtoTypes::number_data_point::Value::AsDouble(d) => format!("{}", d),
                                        OtlpMetricsProtoTypes::number_data_point::Value::AsInt(i) => format!("{}", i),
                                    },
                                    None => "null".to_string(),
                                };
                                
                                let attrs = dp.attributes.iter()
                                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                                    .collect::<Vec<_>>()
                                    .join(",");
                                
                                let one_line_message = format!(
                                    "METRIC [{} {}] name={} type=gauge value={} unit={} resource=[{}] attrs=[{}]",
                                    remote_addr,
                                    format_helpers::format_timestamp_ns(dp.time_unix_nano),
                                    metric.name,
                                    value,
                                    metric.unit,
                                    resource_attrs,
                                    attrs
                                );
                                
                                if self.output_tx.send(one_line_message).await.is_err() {
                                    tracing::error!("Failed to send formatted metric data to output channel");
                                }
                            }
                        }
                        Some(OtlpMetricsProtoTypes::metric::Data::Sum(sum)) => {
                            for dp in &sum.data_points {
                                let value = match &dp.value {
                                    Some(v) => match v {
                                        OtlpMetricsProtoTypes::number_data_point::Value::AsDouble(d) => format!("{}", d),
                                        OtlpMetricsProtoTypes::number_data_point::Value::AsInt(i) => format!("{}", i),
                                    },
                                    None => "null".to_string(),
                                };
                                
                                let attrs = dp.attributes.iter()
                                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                                    .collect::<Vec<_>>()
                                    .join(",");
                                
                                let one_line_message = format!(
                                    "METRIC [{} {}] name={} type=sum value={} unit={} monotonic={} resource=[{}] attrs=[{}]",
                                    remote_addr,
                                    format_helpers::format_timestamp_ns(dp.time_unix_nano),
                                    metric.name,
                                    value,
                                    metric.unit,
                                    sum.is_monotonic,
                                    resource_attrs,
                                    attrs
                                );
                                
                                if self.output_tx.send(one_line_message).await.is_err() {
                                    tracing::error!("Failed to send formatted metric data to output channel");
                                }
                            }
                        }
                        Some(OtlpMetricsProtoTypes::metric::Data::Histogram(hist)) => {
                            for dp in &hist.data_points {
                                let attrs = dp.attributes.iter()
                                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                                    .collect::<Vec<_>>()
                                    .join(",");
                                
                                let one_line_message = format!(
                                    "METRIC [{} {}] name={} type=histogram count={} sum={:?} min={:?} max={:?} unit={} resource=[{}] attrs=[{}]",
                                    remote_addr,
                                    format_helpers::format_timestamp_ns(dp.time_unix_nano),
                                    metric.name,
                                    dp.count,
                                    dp.sum,
                                    dp.min,
                                    dp.max,
                                    metric.unit,
                                    resource_attrs,
                                    attrs
                                );
                                
                                if self.output_tx.send(one_line_message).await.is_err() {
                                    tracing::error!("Failed to send formatted metric data to output channel");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(Response::new(ExportMetricsServiceResponse::default())) // <--- THIS WAS MISSING
    }
}

// ... (OtlpLogsService should be below this) ...
// ... (OtlpLogsService, OtlpTraceService) ...
// --- Logs Service Implementation ---
#[derive(Clone)]
pub struct OtlpLogsService {
    output_tx: TelemetryOutputSender,
    batcher: Option<Arc<Batcher>>,
}

impl OtlpLogsService {
    pub fn new(output_tx: TelemetryOutputSender) -> Self {
        Self { output_tx, batcher: None }
    }

    pub fn with_batcher(mut self, batcher: Arc<Batcher>) -> Self {
        self.batcher = Some(batcher);
        self
    }
}

#[allow(deprecated)]
#[tonic::async_trait]
impl OtelLogsServiceTrait for OtlpLogsService {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let remote_addr = request.remote_addr().map_or_else(
            || "unknown".to_string(),
            |addr| addr.to_string(),
        );

        // Extract index_id from gRPC metadata (headers)
        let index_id = request.metadata().get("index-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(ref idx) = index_id {
            tracing::info!(peer.addr = %remote_addr, index_id = %idx, "Received ExportLogsServiceRequest with index_id");
        } else {
            tracing::info!(peer.addr = %remote_addr, "Received ExportLogsServiceRequest");
        }

        let total_records = request
            .get_ref()
            .resource_logs
            .iter()
            .flat_map(|rl| &rl.scope_logs)
            .flat_map(|sl| &sl.log_records)
            .count();

        // INGEST_LOG_COUNTER.inc_by(total_records as u64);

        let req_data = request.into_inner();

        // If we have a batcher, add logs to batch for storage with index_id
        if let Some(ref batcher) = self.batcher {
            match crate::otlp_ingest_connector::process_otlp_logs_batched_with_index(
                req_data.clone(),
                batcher,
                index_id.clone(),
            ).await {
                Ok(count) => {
                    if let Some(ref idx) = index_id {
                        tracing::info!(index_id = %idx, "Added {} log records to batch with index_id", count);
                    } else {
                        tracing::info!("Added {} log records to batch", count);
                    }
                },
                Err(e) => tracing::error!("Failed to add logs to batch: {}", e),
            }
        }
        
        // Create one-line messages for each log record
        for resource_logs_batch in req_data.resource_logs {
            let resource_attrs = if let Some(resource) = &resource_logs_batch.resource {
                resource.attributes.iter()
                    .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };

            for scope_logs in resource_logs_batch.scope_logs {

                for log_record in scope_logs.log_records {
                    let trace_id_hex = if log_record.trace_id.is_empty() {
                        "".to_string()
                    } else {
                        log_record.trace_id.iter().map(|b| format!("{:02x}",b)).collect::<String>()
                    };
                    
                    let span_id_hex = if log_record.span_id.is_empty() {
                        "".to_string()
                    } else {
                        log_record.span_id.iter().map(|b| format!("{:02x}",b)).collect::<String>()
                    };
                    
                    let severity = match OtlpLogsProtoTypes::SeverityNumber::from_i32(log_record.severity_number) {
                        Some(s) => format!("{:?}", s),
                        None => "Unspecified".to_string()
                    };
                    
                    let attrs = log_record.attributes.iter()
                        .map(|kv| format!("{}={}", kv.key, format_helpers::format_any_value(&kv.value)))
                        .collect::<Vec<_>>()
                        .join(",");
                    
                    let body = format_helpers::format_any_value(&log_record.body);
                    
                    let trace_info = if !trace_id_hex.is_empty() {
                        format!(" trace_id={} span_id={}", trace_id_hex, span_id_hex)
                    } else {
                        String::new()
                    };
                    
                    let one_line_message = format!(
                        "LOG [{} {}] severity={} body={}{} resource=[{}] attrs=[{}]",
                        remote_addr,
                        format_helpers::format_timestamp_ns(log_record.time_unix_nano),
                        severity,
                        body,
                        trace_info,
                        resource_attrs,
                        attrs
                    );
                    
                    if self.output_tx.send(one_line_message).await.is_err() {
                        tracing::error!("Failed to send formatted log data to output channel");
                    }
                }
            }
        }

        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}