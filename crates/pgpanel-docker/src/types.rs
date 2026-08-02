use serde::{Deserialize, Serialize};

/// Spec for creating a managed PostgreSQL container.
#[derive(Debug, Clone)]
pub struct PostgresContainerSpec {
    pub container_name: String,
    pub volume_name: String,
    pub network_name: String,
    pub image: String,
    pub postgres_password: String,
    pub cpu_limit: f64,
    pub memory_mb: u32,
    pub public_port: Option<u16>,
    pub labels: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInspect {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub health: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub memory_limit_mb: f64,
}

#[derive(Debug, Clone)]
pub struct ResourceNames {
    pub container_name: String,
    pub volume_name: String,
    pub network_name: String,
    pub internal_hostname: String,
}
