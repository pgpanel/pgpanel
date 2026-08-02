use std::sync::Arc;

use sqlx::SqlitePool;

use pgpanel_backup::BackupEngine;
use pgpanel_core::config::Config;
use pgpanel_docker::ClusterProvisioner;
use pgpanel_jobs::{JobContext, JobQueue};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub queue: JobQueue,
    pub provisioner: ClusterProvisioner,
    pub backup: Arc<BackupEngine>,
    #[allow(dead_code)]
    pub job_ctx: Arc<JobContext>,
}
