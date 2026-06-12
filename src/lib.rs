//! Federated Learning Client Daemon
//!
//! A secure, production-grade Rust service that enables organizations to participate
//! in collaborative federated learning without exposing raw data.

// Public modules
pub mod config;
pub mod error;
pub mod types;

// Core engine modules
pub mod audit;
pub mod certificates;
pub mod checkpoint;
pub mod metrics;
pub mod model;
pub mod network;
pub mod privacy;
pub mod scheduler;
pub mod secureagg;
pub mod training;

// Re-exports for convenience
pub use error::{DaemonError, Result};
pub use types::*;
