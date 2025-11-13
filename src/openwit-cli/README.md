# OpenWit CLI

A unified command-line interface for OpenWit - a distributed observability platform for ingesting, storing, and querying logs, traces, and metrics.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Commands](#commands)
- [Configuration](#configuration)
- [Service Discovery](#service-discovery)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Development](#development)

## Overview

OpenWit CLI is a single binary that can run different node types for a distributed observability system. Each node type serves a specific purpose in the data pipeline:

- **Control Plane**: Service registry and coordination
- **Ingestion**: Data ingestion via gRPC, HTTP, or Kafka
- **Storage**: Parquet-based storage with Arrow Flight
- **Search**: Query engine for searching and analyzing data
- **Proxy**: Load balancing and request routing
- **Indexer**: Full-text search indexing

## Architecture

```
┌─────────────────┐
│ Control Plane   │  ← Service Registry & Coordination
│   (Port 8001)   │
└────────┬────────┘
         │
    ┌────┴────────────────────────────┐
    │                                 │
┌───▼──────────┐              ┌──────▼──────┐
│   Ingestion  │              │   Storage   │
│ gRPC/HTTP    │─────────────►│  (Parquet)  │
│ (4317/4318)  │              │ (Port 8081) │
└──────┬───────┘              └──────┬──────┘
       │                             │
       │      ┌──────────────┐       │
       └─────►│    Search    │◄──────┘
              │ (Port 8082)  │
              └──────────────┘
```

### Node Types

| Node Type    | Purpose                              | Default Port       |
|-------------|--------------------------------------|--------------------|
| `control`   | Service registry & coordination      | 8001               |
| `ingest`    | Data ingestion (gRPC/HTTP)          | 4317 (gRPC), 4318 (HTTP) |
| `storage`   | Parquet storage, Arrow Flight server | 8081               |
| `search`    | Query engine, log search             | 8082               |
| `proxy`     | Load balancing, request routing      | Auto-assigned      |
| `indexer`   | Full-text search indexing            | 50060              |
| `http`      | HTTP/REST API server                 | 9087               |
| `kafka`     | Kafka consumer for streams           | N/A                |

## Installation

### Build from Source

```bash
# Build release binary with required tokio_unstable flag
RUSTFLAGS="--cfg tokio_unstable" cargo build --bin openwit-cli --release

# Binary will be located at
./target/release/openwit-cli

# Verify installation
./target/release/openwit-cli --version
```

### Development Build

```bash
# Build debug version (faster compilation)
cargo build --bin openwit-cli

# Binary location
./target/debug/openwit-cli
```

## Quick Start

### 1. Generate Sample Configuration

```bash
openwit-cli --create-sample-config
# Creates: config/openwit-unified-control.yaml
```

### 2. Start Control Plane (Required First)

```bash
openwit-cli control
```

The control plane **must** be started first as it provides service discovery for all other nodes.

### 3. Start Other Nodes

```bash
# Terminal 1: Control Plane
openwit-cli control

# Terminal 2: Ingestion (gRPC OTLP)
openwit-cli --ingestion grpc ingest

# Terminal 3: Storage
openwit-cli storage

# Terminal 4: Search
openwit-cli search
```

### 4. Send Test Data

```bash
# Send OTLP trace data to gRPC endpoint
curl -X POST http://localhost:4317/v1/traces \
  -H "Content-Type: application/json" \
  -d '{"resourceSpans":[{"resource":{},"scopeSpans":[]}]}'
```

## Commands

### Global Options

```bash
openwit-cli [OPTIONS] <COMMAND>
```

**Options:**
- `-c, --config <FILE>` - Path to configuration file
- `--create-sample-config` - Generate sample configuration
- `-i, --ingestion <TYPE>` - Ingestion type: `grpc`, `http`, `kafka`, `all`
- `--kafka-brokers <BROKERS>` - Kafka broker addresses (comma-separated)
- `--grpc-port <PORT>` - Override gRPC port
- `--http-port <PORT>` - Override HTTP port
- `-h, --help` - Print help information
- `-V, --version` - Print version

**⚠️ Note:** A command is **required**. Running `openwit-cli` without a command will fail.

### Available Commands

#### `control`
Start the control plane node for service discovery and coordination.

```bash
openwit-cli control [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
  --port <PORT>     Override control plane port (default: 8001)
```

#### `ingest`
Start an ingestion node for receiving telemetry data.

```bash
openwit-cli [--ingestion <TYPE>] ingest [OPTIONS]

Options:
  --node-id <ID>       Custom node identifier
  --port <PORT>        Override ingestion port
  --force-grpc         Force gRPC ingestion
```

**Ingestion Types:**
- `grpc` (default) - gRPC/OTLP protocol
- `http` - HTTP/REST protocol
- `kafka` - Kafka consumer
- `all` - All protocols enabled

#### `storage`
Start a storage node for persisting data to Parquet files.

```bash
openwit-cli storage [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
  --port <PORT>     Override Arrow Flight port (default: 8081)
```

#### `search`
Start a search node for querying stored data.

```bash
openwit-cli search [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
  --port <PORT>     Override search API port (default: 8082)
```

#### `proxy`
Start a proxy node for load balancing across ingestion nodes.

```bash
openwit-cli proxy [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
```

#### `indexer`
Start an indexer node for full-text search.

```bash
openwit-cli indexer [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
  --port <PORT>     Override indexer port (default: 50060)
```

#### `http`
Start an HTTP server node.

```bash
openwit-cli http [OPTIONS]

Options:
  --node-id <ID>    Custom node identifier
  --bind <ADDR>     Bind address (default: 0.0.0.0:9087)
```

#### `kafka`
Start a Kafka consumer node.

```bash
openwit-cli kafka [OPTIONS]

Options:
  --node-id <ID>              Custom node identifier
  --kafka-brokers <BROKERS>   Kafka broker addresses
```

## Configuration

### Configuration File Locations

The CLI searches for configuration files in this order:

1. Path specified via `--config` flag
2. `config/openwit-unified-control.yaml` (project root)
3. `./openwit-unified-control.yaml` (current directory)
4. Parent directories (up to 5 levels up)

### Sample Configuration

```yaml
# OpenWit Unified Configuration
environment: development

deployment:
  mode: distributed  # Options: distributed, standalone
  kubernetes:
    enabled: false
    namespace: openwit
    headless_service: openwit-headless

# Service discovery and networking
networking:
  gossip:
    enabled: false
    listen_addr: "0.0.0.0:7946"
    seeds: []

# Data ingestion configuration
ingestion:
  grpc:
    enabled: true
    bind: "0.0.0.0:4317"
    max_message_size: 4194304  # 4MB

  http:
    enabled: false
    bind: "0.0.0.0:4318"

  kafka:
    enabled: false
    brokers: "localhost:9092"
    topics: [logs, traces, metrics]
    consumer_group: openwit-ingestion

# Storage backend configuration
storage:
  backend: local  # Options: local, s3, azure
  local:
    path: ./data/storage

  s3:
    bucket: openwit-data
    region: us-east-1
    endpoint: null

  azure:
    container: openwit-data
    account_name: ""
    account_key: ""

# Search and indexing
indexing:
  enabled: true
  backend: tantivy
  path: ./data/index
  settings:
    max_merge_threads: 2
    commit_interval_seconds: 10

search:
  enabled: true
  bind: "0.0.0.0:9001"
  cache:
    enabled: true
    size_mb: 256
    ttl_seconds: 300

# Metadata storage
metastore:
  backend: sled  # Options: sled, postgres
  sled:
    path: ./data/metastore
  postgres:
    connection_string: "postgresql://user:password@localhost/openwit"
    max_connections: 10

# Observability for OpenWit itself
observability:
  prometheus:
    enabled: true
    bind: "0.0.0.0:9090"
  tracing:
    enabled: false
    endpoint: "http://localhost:4318"
    service_name: openwit

# Performance tuning
performance:
  buffer_size_mb: 256
  batch_size: 1000
  batch_timeout_ms: 100
  wal_enabled: true
  wal_path: ./data/wal
```

### Environment Variable Overrides

Override configuration with environment variables:

```bash
# Override environment
export OPENWIT_ENVIRONMENT=production

# Override deployment mode
export OPENWIT_DEPLOYMENT_MODE=distributed

# Override ingestion settings
export OPENWIT_INGESTION_GRPC_ENABLED=true

# Override storage backend
export OPENWIT_STORAGE_BACKEND=azure
```

## Service Discovery

### Local Development (File-Based)

In local mode, nodes register themselves in `./data/.openwit_services.json`:

```json
{
  "control": [{
    "node_id": "control-hostname-abc123",
    "endpoint": "http://localhost:8001",
    "service_port": 8001
  }],
  "ingest": [{
    "node_id": "ingest-hostname-def456",
    "endpoint": "http://localhost:4317",
    "service_port": 4317
  }],
  "storage": [{
    "node_id": "storage-hostname-ghi789",
    "endpoint": "http://localhost:8081",
    "service_port": 8081
  }]
}
```

### Kubernetes (DNS-Based)

In Kubernetes, nodes discover each other via DNS:

```yaml
control_plane:
  grpc_endpoint: http://openwit-control.openwit.svc.cluster.local:8001
```

### Automatic Port Allocation

**Local Mode:**
- Automatically finds free ports if default port is busy
- Example: Control tries 8001 → 8002 → 8003...

**Kubernetes Mode:**
- Uses fixed ports from configuration
- Relies on Kubernetes service discovery

## Examples

### Example 1: Full Local Setup

```bash
# Terminal 1: Control Plane
openwit-cli control --node-id control-1

# Terminal 2: gRPC Ingestion
openwit-cli --ingestion grpc ingest --node-id ingest-1

# Terminal 3: Storage
openwit-cli storage --node-id storage-1

# Terminal 4: Search
openwit-cli search --node-id search-1

# Terminal 5: Send test data
curl -X POST http://localhost:4318/v1/traces \
  -H "Content-Type: application/json" \
  -d '{"resourceSpans":[{"resource":{},"scopeSpans":[]}]}'
```

### Example 2: Kubernetes Deployment

```yaml
# Control Plane Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: openwit-control
spec:
  template:
    spec:
      containers:
      - name: openwit-control
        image: openwit:latest
        command:
          - openwit-cli
          - control
          - --config=/config/openwit.yaml
          - --node-id=$(HOSTNAME)
        env:
          - name: HOSTNAME
            valueFrom:
              fieldRef:
                fieldPath: metadata.name
---
# Ingestion Deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: openwit-ingest
spec:
  replicas: 3  # Horizontal scaling
  template:
    spec:
      containers:
      - name: openwit-ingest
        image: openwit:latest
        command:
          - openwit-cli
          - --ingestion=grpc
          - ingest
          - --config=/config/openwit.yaml
          - --node-id=$(HOSTNAME)
```

### Example 3: Kafka Consumer

```bash
# Start Kafka consumer for streaming ingestion
openwit-cli \
  --ingestion kafka \
  --kafka-brokers kafka-1:9092,kafka-2:9092,kafka-3:9092 \
  kafka \
  --node-id kafka-consumer-1
```

### Example 4: Custom Port Configuration

```bash
# Override default ports
openwit-cli \
  --config /custom/path/config.yaml \
  --ingestion grpc \
  --grpc-port 5000 \
  ingest \
  --node-id custom-ingest
```

### Example 5: Multi-Protocol Ingestion

```bash
# Start ingestion with all protocols enabled
openwit-cli \
  --ingestion all \
  ingest \
  --node-id multi-protocol-ingest
```

## Troubleshooting

### Missing Subcommand Error

**Problem:**
```bash
$ openwit-cli
error: A subcommand is required
```

**Solution:**
```bash
# Always specify which node type to run
openwit-cli control
openwit-cli ingest
openwit-cli storage
```

### Configuration Not Found

**Problem:**
```
Error: Configuration file not found
```

**Solution:**
```bash
# Generate sample configuration
openwit-cli --create-sample-config

# Or specify config explicitly
openwit-cli --config /path/to/config.yaml control
```

### Port Already in Use

**Problem:**
```
Error: Address already in use (port 8001)
```

**Solution:**
```bash
# In local mode, CLI auto-finds next available port
# Just restart the node

# Or specify custom port
openwit-cli control --port 8002
```

### Service Discovery Issues

**Problem:**
Nodes cannot find each other

**Solution:**
```bash
# 1. Check service registry
cat ./data/.openwit_services.json | jq

# 2. Ensure control plane is running first
openwit-cli control

# 3. Check control plane is reachable
curl http://localhost:8001/status

# 4. Verify node registration
cat ./data/.openwit_services.json | jq '.control'
```

### Kafka Connection Failures

**Problem:**
```
Error: Failed to connect to Kafka brokers
```

**Solution:**
```bash
# 1. Verify Kafka brokers are running
telnet localhost 9092

# 2. Check broker addresses
openwit-cli --kafka-brokers localhost:9092 kafka

# 3. Verify topics exist in Kafka
kafka-topics --list --bootstrap-server localhost:9092
```

### gRPC Connection Errors

**Problem:**
```
Error: Failed to send batch to storage
```

**Solution:**
```bash
# 1. Check storage node is running
curl http://localhost:8081/health

# 2. Check service registry
cat ./data/.openwit_services.json | jq '.storage'

# 3. Enable debug logging
RUST_LOG=debug openwit-cli ingest
```

## Monitoring and Operations

### Health Checks

```bash
# Control plane health
curl http://localhost:8001/health
curl http://localhost:8001/status

# Search API health
curl http://localhost:8082/api/health

# Storage Arrow Flight health
grpcurl -plaintext localhost:8081 list
```

### View Service Registry

```bash
# Pretty print service registry
cat ./data/.openwit_services.json | jq

# Watch for changes
watch -n 1 'cat ./data/.openwit_services.json | jq'
```

### Logging

```bash
# Enable debug logging
RUST_LOG=debug openwit-cli control

# Module-specific logging
RUST_LOG=openwit_cli=debug,openwit_ingestion=trace openwit-cli ingest

# Info level only
RUST_LOG=info openwit-cli storage

# Log to file
RUST_LOG=debug openwit-cli control 2>&1 | tee control.log
```

### Graceful Shutdown

Press `Ctrl+C` to gracefully shutdown. The node will:
1. Stop accepting new requests
2. Flush pending data to storage
3. Unregister from service registry
4. Close all connections
5. Exit cleanly

## Performance Tuning

### Horizontal Scaling

```bash
# Scale ingestion nodes
openwit-cli --ingestion grpc ingest --node-id ingest-1 &
openwit-cli --ingestion grpc ingest --node-id ingest-2 &
openwit-cli --ingestion grpc ingest --node-id ingest-3 &

# Scale storage nodes
openwit-cli storage --node-id storage-1 &
openwit-cli storage --node-id storage-2 &

# Proxy will automatically load balance across all nodes
openwit-cli proxy --node-id proxy-1
```

### Configuration Tuning

```yaml
performance:
  buffer_size_mb: 512      # Increase for high throughput
  batch_size: 5000         # Larger batches = better throughput
  batch_timeout_ms: 200    # Higher latency, better batching
  wal_enabled: true        # Write-ahead log for durability
```

### Resource Limits

```yaml
ingestion:
  grpc:
    max_message_size: 8388608  # 8MB for large traces
    max_concurrent_streams: 1000

storage:
  local:
    max_buffer_size_mb: 1024  # 1GB buffer
    flush_interval_seconds: 60
```

## Development

### Module Structure

```
openwit-cli/
├── src/
│   ├── main.rs                    # CLI entry point, argument parsing
│   ├── config_loader.rs           # Configuration loading with overrides
│   ├── ingestion_type.rs          # Ingestion type enum and logic
│   ├── node_startup.rs            # Startup logic for each node type
│   └── distributed_auto_config.rs # Service discovery and registration
├── Cargo.toml                     # Dependencies and build config
└── README.md                      # This file
```

### Building from Source

```bash
# Development build (fast compilation, no optimizations)
cargo build --bin openwit-cli

# Release build (optimized, slower compilation)
RUSTFLAGS="--cfg tokio_unstable" cargo build --bin openwit-cli --release

# Build with verbose output
cargo build --bin openwit-cli --release -vv
```

### Running Tests

```bash
# Run all tests
cargo test -p openwit-cli

# Run with output
cargo test -p openwit-cli -- --nocapture

# Run specific test
cargo test -p openwit-cli test_config_loader_creation
```

### Code Quality

```bash
# Check for errors
cargo check -p openwit-cli

# Run clippy linter
cargo clippy -p openwit-cli

# Format code
cargo fmt -p openwit-cli

# Run all quality checks
cargo check && cargo clippy && cargo fmt --check
```

## Key Features

### 1. Smart Configuration Loading
- Searches multiple locations automatically
- Environment variable overrides
- Validation before startup
- Sample config generation

### 2. Automatic Service Discovery
- File-based registry for local development
- DNS-based discovery for Kubernetes
- Automatic port allocation
- Graceful degradation

### 3. Flexible Deployment
- Single binary for all node types
- Horizontal scaling support
- Zero-downtime updates
- Kubernetes-native

### 4. Production Ready
- Graceful shutdown
- Health check endpoints
- Comprehensive logging
- Error recovery

### 5. Developer Friendly
- Clear error messages
- Helpful defaults
- Extensive documentation
- Sample configurations

## Architecture Patterns

### Service Registry Pattern
Centralized service discovery via control plane allows dynamic node registration and discovery.

### Port Allocation Strategy
- **Local**: Dynamic port finding prevents conflicts
- **Kubernetes**: Static ports with DNS-based discovery

### Node Identification
Auto-generated format: `{type}-{hostname}-{short-uuid}`

Example: `ingest-macbook-a1b2c3`

Override: `--node-id custom-name`

## License

See the main repository LICENSE file.

## Support

- **GitHub Issues**: [Report bugs or request features](https://github.com/middleware-labs/openwit/issues)
- **Documentation**: [Full documentation](https://docs.openwit.io)
- **Community**: Join our community discussions

---

## Quick Reference Card

```bash
# Generate config
openwit-cli --create-sample-config

# Start control plane (required first!)
openwit-cli control

# Start ingestion (gRPC OTLP)
openwit-cli --ingestion grpc ingest

# Start storage
openwit-cli storage

# Start search
openwit-cli search

# View all options
openwit-cli --help

# View command-specific options
openwit-cli control --help
openwit-cli ingest --help

# Enable debug logging
RUST_LOG=debug openwit-cli <command>

# Check service registry
cat ./data/.openwit_services.json | jq
```

**Remember:** Always start the control plane first! 🚀
