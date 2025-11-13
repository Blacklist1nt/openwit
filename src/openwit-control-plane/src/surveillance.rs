use std::sync::Arc;
// use std::time::Duration; // Removed - was only used for gossip monitoring
// use std::collections::HashMap; // Removed - was only used for gossip monitoring
use anyhow::Result;
// use tokio::time::interval; // Removed - was only used for gossip monitoring
use tracing::info;

// use openwit_network::ClusterHandle; // Network/gossip removed
use openwit_config::UnifiedConfig;
use crate::types::*;
use crate::node_manager::NodeManager;
// use crate::mandatory_node_checker::MandatoryNodeChecker; // Removed - depends on network/gossip

pub struct SurveillanceNode {
    pub node_id: NodeId,
    // pub cluster_handle: ClusterHandle, // Network/gossip removed
    pub node_manager: Arc<NodeManager>,
    // pub mandatory_checker: Option<Arc<MandatoryNodeChecker>>, // Removed - depends on network/gossip
    pub config: Option<UnifiedConfig>,
}

impl SurveillanceNode {
    pub async fn new(
        node_id: String,
        // cluster_handle parameter removed - gossip/network deprecated
    ) -> Result<Self> {
        let node_manager = Arc::new(NodeManager::new(300)); // 5 minutes heartbeat timeout for registration-only nodes

        Ok(Self {
            node_id: NodeId::new(node_id),
            // cluster_handle removed - gossip/network deprecated
            node_manager,
            // mandatory_checker: None, // Removed - depends on network/gossip
            config: None,
        })
    }
    
    
    /// Set configuration for mandatory node checking (DISABLED - network removed)
    pub fn with_config(mut self, config: UnifiedConfig) -> Self {
        // Mandatory node checking disabled - requires gossip/network which was removed
        // if let Some(mandatory_nodes) = config.mandatory_nodes.clone() {
        //     let mandatory_checker = MandatoryNodeChecker::new(
        //         mandatory_nodes,
        //         self.cluster_handle.clone(),
        //     );
        //     self.mandatory_checker = Some(Arc::new(mandatory_checker));
        // }
        self.config = Some(config);
        self
    }
    
    pub async fn start(&self) -> Result<()> {
        info!("🔍 Starting surveillance node: {}", self.node_id.0);
        info!("   Initializing subsystems...");

        // Check mandatory nodes first if configured (DISABLED - network removed)
        // if let Some(checker) = &self.mandatory_checker {
        //     checker.verify_mandatory_nodes().await?;
        // }

        // Register self as control node
        let self_metadata = NodeMetadata {
            id: self.node_id.clone(),
            role: NodeRole::Control,
            state: NodeState::Running,
            last_heartbeat: chrono::Utc::now(),
            metadata: Default::default(),
        };
        
        self.node_manager.register_node(self_metadata).await?;
        info!("   ✓ Control node registered");
        
        // Start subsystems
        self.node_manager.start_health_monitor().await;
        info!("   ✓ Health monitor started");

        // Start cluster state monitor (DISABLED - gossip/network removed)
        // self.start_cluster_monitor().await;
        // info!("   ✓ Cluster state monitor started");

        info!("");
        info!("Surveillance node {} ready", self.node_id.0);

        Ok(())
    }

    // REMOVED: Gossip-based cluster monitoring (network removed)
    // async fn start_cluster_monitor(&self) {
    //     let cluster = self.cluster_handle.clone();
    //     let node_manager = self.node_manager.clone();
    //
    //     tokio::spawn(async move {
    //         let mut ticker = interval(Duration::from_secs(5));
    //
    //         loop {
    //             ticker.tick().await;
    //
    //             // Get cluster state from gossip
    //             let roles = cluster.view.get_roles().await;
    //
    //             // Update node registry based on gossip state
    //             for (node_id, role) in roles {
    //                 // Convert role string to NodeRole
    //                 let node_role = match role.as_str() {
    //                     "control" => NodeRole::Control,
    //                     "ingest" => NodeRole::Ingest,
    //                     "search" => NodeRole::Search,
    //                     "storage" => NodeRole::Storage,
    //                     "query" => NodeRole::Query,
    //                     "kafka" => NodeRole::Kafka,
    //                     "monolith" => NodeRole::Hybrid(vec![
    //                         NodeRole::Control,
    //                         NodeRole::Ingest,
    //                         NodeRole::Search,
    //                         NodeRole::Storage,
    //                         NodeRole::Query,
    //                     ]),
    //                     _ => continue,
    //                 };
    //
    //                 // Register or update node
    //                 let mut node_metadata = HashMap::new();
    //                 // Set service_type based on role
    //                 let service_type = match &node_role {
    //                     NodeRole::Control => "control",
    //                     NodeRole::Ingest => "ingest",  // Fixed: was "grpc", should be "ingest"
    //                     NodeRole::Search => "search",
    //                     NodeRole::Storage => "storage",
    //                     NodeRole::Query => "query",
    //                     NodeRole::Kafka => "kafka",
    //                     NodeRole::Hybrid(_) => "monolith",
    //                 };
    //                 node_metadata.insert("service_type".to_string(), service_type.to_string());
    //
    //                 let metadata = NodeMetadata {
    //                     id: NodeId::new(node_id),
    //                     role: node_role,
    //                     state: NodeState::Running,
    //                     last_heartbeat: chrono::Utc::now(),
    //                     metadata: node_metadata,
    //                 };
    //
    //                 let _ = node_manager.register_node(metadata).await;
    //             }
    //         }
    //     });
    // }

}