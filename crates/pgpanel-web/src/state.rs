//! Shared application state.

use crate::helper_client::HelperClient;
use crate::services::audit_service::AuditService;
use crate::services::monitoring::MonitoringService;
use crate::services::postgres_admin::PostgresAdmin;
use crate::services::session::SessionService;
use crate::services::updates_service::UpdatesService;
use pgpanel_core::auth::SecretKey;
use pgpanel_core::config::Config;
use sqlx::SqlitePool;
use std::sync::Arc;

/// Global application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: SqlitePool,
    pub secret_key: Arc<SecretKey>,
    pub helper: HelperClient,
    pub sessions: SessionService,
    pub audit: AuditService,
    pub postgres: PostgresAdmin,
    pub monitoring: MonitoringService,
    pub updates: Arc<UpdatesService>,
}

impl AppState {
    /// Build state from config and database pool.
    pub fn new(config: Config, db: SqlitePool) -> Result<Self, pgpanel_core::CoreError> {
        let secret_key = Arc::new(SecretKey::new(config.app.secret_key.clone())?);
        let config = Arc::new(config);
        let helper = HelperClient::new(&config);
        let sessions = SessionService::new(db.clone(), &config);
        let audit = AuditService::new(db.clone());
        let postgres = PostgresAdmin::new(db.clone(), secret_key.clone(), config.clone());
        let monitoring = MonitoringService::new(db.clone(), config.clone());
        let updates = Arc::new(UpdatesService::new(config.clone()));

        Ok(Self {
            config,
            db,
            secret_key,
            helper,
            sessions,
            audit,
            postgres,
            monitoring,
            updates,
        })
    }
}
