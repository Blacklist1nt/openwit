// OpenWit Inter-Node Communication
//
// This module provides Arrow Flight-based data transfer between nodes
// and node address caching with round-robin load balancing.

pub mod arrow_flight_service;
pub mod address_cache;

// Re-export Arrow Flight components
pub use arrow_flight_service::{
    ArrowFlightService,
    ArrowFlightClient,
    ArrowFlightBatch,
    get_flight_port,
};

// Re-export address cache
pub use address_cache::{
    NodeAddressCache,
    NodeAddress,
    parse_node_endpoint,
};
