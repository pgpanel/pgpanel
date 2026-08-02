//! Restricted Docker operations for PostgreSQL cluster provisioning.
//!
//! Only allowlisted images, fixed mount paths, and controlled environment
//! variables are permitted. Arbitrary Docker commands are never accepted.
#![forbid(unsafe_code)]

mod client;
mod cluster;
mod types;

pub use client::DockerClient;
pub use cluster::{ClusterProvisioner, ProvisionedResources};
pub use types::*;
