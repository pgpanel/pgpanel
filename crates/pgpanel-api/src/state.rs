use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use pgpanel_backup::BackupEngine;
use pgpanel_core::config::Config;
use pgpanel_core::models::WafPolicyConfig;
use pgpanel_docker::{ClusterProvisioner, NodeRegistry};
use pgpanel_jobs::{JobContext, JobQueue};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub queue: JobQueue,
    pub provisioner: ClusterProvisioner,
    pub backup: Arc<BackupEngine>,
    pub nodes: NodeRegistry,
    /// Hot-reloaded active WAF policy (IP deny, UA block, body size, etc.).
    pub waf: Arc<RwLock<WafPolicyConfig>>,
    #[allow(dead_code)]
    pub job_ctx: Arc<JobContext>,
}
