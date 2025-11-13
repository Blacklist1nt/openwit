use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use tokio::time::{interval};
use tracing::{info, error, warn, debug};
use tokio::sync::RwLock;
use openwit_config::UnifiedConfig;

/// Manages HTTP server's integration with the control plane
pub struct ControlPlaneIntegration {
    node_id: String,
    node_role: String,
    bind_addr: String,
    config: Arc<UnifiedConfig>,
    client: Arc<RwLock<Option<openwit_control_plane::client::ControlPlaneClient>>>,
}

impl ControlPlaneIntegration {
    /// Create a new control plane integration (without immediate connection)
    pub async fn new(
        node_id: String,
        bind_addr: String,
        config: Arc<UnifiedConfig>,
    ) -> Result<Self> {
        info!("Creating control plane integration for {}", config.control_plane.grpc_endpoint);

        Ok(Self {
            node_id,
            node_role: "ingest".to_string(), // HTTP servers are ingest nodes
            bind_addr,
            config,
            client: Arc::new(RwLock::new(None)),
        })
    }
    
    /// Connect to control plane with retries
    async fn ensure_connected(&self) -> Result<()> {
        let mut client_guard = self.client.write().await;

        // If already connected, return
        if client_guard.is_some() {
            return Ok(());
        }

        // Try to connect using ControlPlaneClient
        info!("Attempting to connect to control plane at {}", self.config.control_plane.grpc_endpoint);

        match openwit_control_plane::client::ControlPlaneClient::new(&self.node_id, &self.config).await {
            Ok(client) => {
                *client_guard = Some(client);
                info!("Successfully connected to control plane");
                Ok(())
            }
            Err(e) => {
                debug!("Failed to connect to control plane: {}", e);
                Err(anyhow::anyhow!("Connection failed: {}", e))
            }
        }
    }
    
    /// Start periodic health reporting to control plane with reconnection logic
    pub async fn start_health_reporting(self: Arc<Self>) {
        let mut ticker = interval(Duration::from_secs(5)); // Report every 5 seconds
        interval(Duration::from_secs(30)); // Try reconnect every 30 seconds
        let mut consecutive_failures = 0;
        
        info!("Starting health reporting for HTTP node {}", self.node_id);
        
        loop {
            ticker.tick().await;
            
            // First ensure we're connected
            if self.ensure_connected().await.is_err() {
                consecutive_failures += 1;
                if consecutive_failures % 6 == 0 { // Log every 30 seconds
                    warn!("Still unable to connect to control plane after {} attempts", consecutive_failures);
                }
                continue;
            }
            
            // If we just reconnected, reset failure count
            if consecutive_failures > 0 {
                info!("Reconnected to control plane after {} failures", consecutive_failures);
                consecutive_failures = 0;
            }
            
            // Try to report health
            if let Err(e) = self.report_health().await {
                error!("Failed to report health: {}", e);
                // Mark client as disconnected so we'll reconnect next time
                let mut client_guard = self.client.write().await;
                *client_guard = None;
            }
        }
    }
    
    /// Report node health to control plane by re-registering (acts as heartbeat)
    async fn report_health(&self) -> Result<()> {
        let mut client_guard = self.client.write().await;

        if let Some(ref mut client) = *client_guard {
            // Build HTTP endpoint
            let http_endpoint = self.get_http_endpoint();

            // Create registration metadata
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("http_endpoint".to_string(), http_endpoint.clone());
            metadata.insert("status".to_string(), "accepting".to_string());
            metadata.insert("service_type".to_string(), "http".to_string());

            // Extract port from bind_addr
            if let Some(port) = self.bind_addr.split(':').last() {
                metadata.insert("http_port".to_string(), port.to_string());
            }

            // Add pod information if in Kubernetes
            if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
                if let Ok(pod_ip) = std::env::var("POD_IP") {
                    metadata.insert("pod_ip".to_string(), pod_ip);
                }
                if let Ok(pod_name) = std::env::var("HOSTNAME") {
                    metadata.insert("pod_name".to_string(), pod_name);
                }
            }

            // Register with control plane (repeated calls act as heartbeat)
            match client.register_node(&self.node_id, &self.node_role, metadata).await {
                Ok(_) => {
                    debug!("✅ HTTP node {} heartbeat sent", self.node_id);
                    Ok(())
                }
                Err(e) => {
                    warn!("⚠️ Failed to send heartbeat for HTTP node {}: {}", self.node_id, e);
                    Err(e)
                }
            }
        } else {
            Err(anyhow::anyhow!("Not connected to control plane"))
        }
    }

    /// Build HTTP endpoint for registration
    fn get_http_endpoint(&self) -> String {
        // Determine the correct HTTP endpoint based on environment
        if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
            // In Kubernetes, use service name or pod IP
            if let Ok(service_name) = std::env::var("HTTP_SERVICE_NAME") {
                info!("Using Kubernetes service name for HTTP endpoint: {}", service_name);
                return format!("http://{}", service_name);
            } else {
                // Fallback to pod IP
                let pod_ip = std::env::var("POD_IP").unwrap_or_else(|_| "localhost".to_string());
                warn!("No HTTP service name configured, using pod IP: {}", pod_ip);
                return format!("http://{}", self.bind_addr.replace("0.0.0.0", &pod_ip));
            }
        }

        // Local/non-Kubernetes: use advertise address
        let advertise_addr = self.config.networking.get_service_advertise_address("http");

        // Extract port from bind_addr
        let port = self.bind_addr.split(':').last().unwrap_or("4318");
        format!("http://{}:{}", advertise_addr, port)
    }
}