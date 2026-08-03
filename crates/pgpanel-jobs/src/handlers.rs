use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use pgpanel_backup::{
    collect_segments, enable_archiving, pg_basebackup_tar, query_wal_status, switch_wal,
    BackupEngine, RunLogicalBackupRequest,
};
use pgpanel_core::config::Config;
use pgpanel_core::crypto::{decrypt_secret, encrypt_secret, generate_password};
use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::{
    ClusterStatus, CreateClusterRequest, CreateDatabaseRequest, DatabasusIntegrationStatus,
    DeleteMode, HealthStatus, JobType, Operation,
};
use pgpanel_docker::{ClusterProvisioner, NodeRegistry};
use pgpanel_postgres::{PgClient, RoleService};

use crate::queue::JobQueue;

pub struct JobContext {
    pub queue: JobQueue,
    pub pool: SqlitePool,
    pub config: Config,
    pub provisioner: ClusterProvisioner,
    pub backup: Arc<BackupEngine>,
    pub nodes: NodeRegistry,
}

impl JobContext {
    pub async fn handle(&self, op: Operation) -> Result<serde_json::Value> {
        match op.job_type {
            JobType::CreateCluster => self.handle_create_cluster(&op).await,
            JobType::StartCluster => self.handle_lifecycle(&op, "start").await,
            JobType::StopCluster => self.handle_lifecycle(&op, "stop").await,
            JobType::RestartCluster => self.handle_lifecycle(&op, "restart").await,
            JobType::DeleteCluster => self.handle_delete(&op).await,
            JobType::EnableBackup | JobType::RegisterDatabasus => {
                self.handle_enable_backup(&op).await
            }
            JobType::RunBackup => self.handle_run_backup(&op).await,
            JobType::VerifyBackup => self.handle_verify_backup(&op).await,
            JobType::RestoreBackup => self.handle_restore_backup(&op).await,
            JobType::PruneBackups => self.handle_prune_backups(&op).await,
            JobType::SyncWal => self.handle_sync_wal(&op).await,
            JobType::WalBaseBackup => self.handle_wal_base_backup(&op).await,
            JobType::WalPitrRestore => self.handle_wal_pitr_restore(&op).await,
            JobType::CreateDatabase => self.handle_create_database(&op).await,
            JobType::DeleteDatabase => self.handle_delete_database(&op).await,
            JobType::RotatePassword => self.handle_rotate_password(&op).await,
            JobType::UpdateNetworking => self.handle_update_networking(&op).await,
            JobType::RefreshMetrics => self.handle_refresh_metrics(&op).await,
            JobType::PingNode => self.handle_ping_node(&op).await,
            JobType::SyncReplica => self.handle_sync_replica(&op).await,
            JobType::PromoteReplica => self.handle_promote_replica(&op).await,
            JobType::DuplicateCluster => self.handle_duplicate_cluster(&op).await,
            JobType::EvaluateAlerts => self.handle_evaluate_alerts(&op).await,
        }
    }

    /// Provisioner bound to the Docker host of the cluster's node.
    async fn provisioner_for_cluster(&self, cluster_id: Uuid) -> Result<ClusterProvisioner> {
        let node_id: Option<String> =
            sqlx::query_scalar("SELECT node_id FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?
                .flatten();
        let Some(nid) = node_id else {
            return Ok(self.provisioner.clone());
        };
        let row = sqlx::query_as::<_, (String, Option<String>, i64)>(
            "SELECT kind, docker_host, docker_host_encrypted FROM nodes WHERE id = ?",
        )
        .bind(&nid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        let Some((kind, host, enc)) = row else {
            return Ok(self.provisioner.clone());
        };
        if kind == "local" {
            return Ok(self.provisioner.clone());
        }
        let host = if enc != 0 {
            let enc = host.ok_or_else(|| Error::Internal("missing docker_host".into()))?;
            let plain = decrypt_secret(&self.config.master_encryption_key, &enc)?;
            plain.expose_secret().to_string()
        } else {
            host.unwrap_or_default()
        };
        let node_uuid = Uuid::parse_str(&nid).unwrap_or_else(|_| NodeRegistry::local_node_id());
        let docker = self.nodes.client_for_host(node_uuid, Some(&host)).await?;
        Ok(self.provisioner.with_docker(docker))
    }

    async fn handle_create_cluster(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let req: CreateClusterRequest = serde_json::from_value(op.payload.clone())
            .map_err(|e| Error::Job(format!("payload: {e}")))?;

        self.queue
            .set_progress(op.id, 5, "loading cluster record")
            .await?;

        // Load encrypted admin password from credentials
        let password = self.load_admin_password(cluster_id).await?;

        let slug: String = sqlx::query_scalar("SELECT slug FROM clusters WHERE id = ?")
            .bind(cluster_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;

        self.queue
            .set_progress(op.id, 15, "provisioning Docker resources")
            .await?;

        self.set_cluster_status(cluster_id, ClusterStatus::Creating, None)
            .await?;

        let (names, resources) = match self
            .provisioner_for_cluster(cluster_id)
            .await?
            .provision(cluster_id, &slug, &req, password.expose_secret())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "provision failed");
                self.queue
                    .append_log(
                        op.id,
                        "error",
                        &format!("provision failed (data volume retained if created): {e}"),
                    )
                    .await
                    .ok();
                self.set_cluster_status(cluster_id, ClusterStatus::Failed, Some(&e.to_string()))
                    .await?;
                return Err(e);
            }
        };

        self.queue
            .append_log(
                op.id,
                "info",
                &format!(
                    "resources: volume={} network={} container={} started={}",
                    resources.volume_created,
                    resources.network_created,
                    resources.container_created,
                    resources.container_started
                ),
            )
            .await?;

        if let Some(cid) = &resources.container_id {
            sqlx::query(
                "UPDATE clusters SET docker_container_id = ?, docker_container_name = ?, docker_volume_name = ?, docker_network_name = ?, internal_hostname = ?, updated_at = ? WHERE id = ?",
            )
            .bind(cid)
            .bind(&names.container_name)
            .bind(&names.volume_name)
            .bind(&names.network_name)
            .bind(&names.internal_hostname)
            .bind(Utc::now().to_rfc3339())
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;
        }

        self.queue
            .set_progress(op.id, 50, "waiting for PostgreSQL healthcheck")
            .await?;
        self.set_cluster_status(cluster_id, ClusterStatus::Starting, None)
            .await?;

        self.provisioner
            .wait_healthy(&names.container_name)
            .await
            .map_err(|e| {
                // Do NOT delete volume on health failure
                error!(error = %e, "health wait failed; volume retained");
                e
            })?;

        // Connect to postgres and optionally create backup role
        self.queue
            .set_progress(op.id, 70, "configuring PostgreSQL")
            .await?;

        let pg = self
            .connect_cluster_admin(cluster_id, &names.internal_hostname)
            .await;

        let mut created_roles: Vec<serde_json::Value> = Vec::new();

        if let Ok(ref client) = pg {
            let roles = RoleService::new(client);

            if req.enable_backup {
                let backup_pw = generate_password();
                if let Err(e) = roles.create_backup_role(&backup_pw).await {
                    self.queue
                        .append_log(op.id, "warn", &format!("backup role: {e}"))
                        .await?;
                } else {
                    let enc = encrypt_secret(&self.config.master_encryption_key, &backup_pw)?;
                    self.store_credential(cluster_id, "pgpanel_backup", "backup", &enc)
                        .await?;
                }
            }

            // Create user-specified databases + roles from the create form
            let cluster_slug: String =
                sqlx::query_scalar("SELECT slug FROM clusters WHERE id = ?")
                    .bind(cluster_id.to_string())
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or_else(|_| "app".into());

            for spec in &req.initial_databases {
                let role_name = spec
                    .role_name
                    .clone()
                    .unwrap_or_else(|| cluster_slug.clone());
                self.queue
                    .append_log(
                        op.id,
                        "info",
                        &format!(
                            "creating initial database {} owner {}",
                            spec.database_name, role_name
                        ),
                    )
                    .await?;
                let password = if let Some(ref p) = spec.password {
                    if let Err(e) = pgpanel_core::crypto::validate_password_strength(p) {
                        self.queue
                            .append_log(
                                op.id,
                                "warn",
                                &format!("password for {role_name}: {e}"),
                            )
                            .await?;
                        generate_password()
                    } else {
                        SecretString::from(p.clone())
                    }
                } else {
                    generate_password()
                };

                if let Err(e) = roles.create_role(&role_name, &password, None).await {
                    self.queue
                        .append_log(
                            op.id,
                            "error",
                            &format!("create role {role_name}: {e}"),
                        )
                        .await?;
                    continue;
                }
                if let Err(e) = roles
                    .create_database(&spec.database_name, &role_name, None)
                    .await
                {
                    self.queue
                        .append_log(
                            op.id,
                            "error",
                            &format!("create database {}: {e}", spec.database_name),
                        )
                        .await?;
                    continue;
                }

                let enc = encrypt_secret(&self.config.master_encryption_key, &password)?;
                self.store_credential(cluster_id, &role_name, "app", &enc)
                    .await?;

                let now = Utc::now().to_rfc3339();
                let db_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO databases (id, cluster_id, name, owner_role, connection_limit, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
                )
                .bind(db_id.to_string())
                .bind(cluster_id.to_string())
                .bind(&spec.database_name)
                .bind(&role_name)
                .bind(&now)
                .execute(&self.pool)
                .await;

                let role_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO database_roles (id, cluster_id, name, is_superuser, can_login, connection_limit, created_at) VALUES (?, ?, ?, 0, 1, NULL, ?)",
                )
                .bind(role_id.to_string())
                .bind(cluster_id.to_string())
                .bind(&role_name)
                .bind(&now)
                .execute(&self.pool)
                .await;

                created_roles.push(serde_json::json!({
                    "database": spec.database_name,
                    "role": role_name,
                    "password": password.expose_secret(),
                }));
            }
        } else if let Err(e) = &pg {
            self.queue
                .append_log(op.id, "warn", &format!("admin connect after start: {e}"))
                .await?;
        }

        // Native backup schedule (failure must not kill the cluster)
        if req.enable_backup {
            self.queue
                .set_progress(op.id, 85, "configuring native backups")
                .await?;
            if let Err(e) = self
                .enable_backup_inner(cluster_id, &names.container_name)
                .await
            {
                self.queue
                    .append_log(op.id, "warn", &format!("backup setup: {e}"))
                    .await?;
                self.set_cluster_status(
                    cluster_id,
                    ClusterStatus::HealthyWithBackupWarning,
                    Some(&format!("backup setup failed: {e}")),
                )
                .await?;
                self.set_health(cluster_id, HealthStatus::Healthy).await?;
                return Ok(serde_json::json!({
                    "cluster_id": cluster_id,
                    "status": "healthy_with_backup_warning",
                    "warning": e.to_string(),
                    "initial_credentials": created_roles,
                }));
            }
        }

        self.set_cluster_status(cluster_id, ClusterStatus::Healthy, None)
            .await?;
        self.set_health(cluster_id, HealthStatus::Healthy).await?;
        self.queue
            .set_progress(op.id, 100, "cluster healthy")
            .await?;

        Ok(serde_json::json!({
            "cluster_id": cluster_id,
            "status": "healthy",
            "container": names.container_name,
            "initial_credentials": created_roles,
        }))
    }

    async fn handle_lifecycle(&self, op: &Operation, action: &str) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::NotFound(format!("cluster: {e}")))?;

        let provisioner = self.provisioner_for_cluster(cluster_id).await?;

        match action {
            "start" => {
                self.set_cluster_status(cluster_id, ClusterStatus::Starting, None)
                    .await?;
                provisioner.start(&container).await?;
                self.set_cluster_status(cluster_id, ClusterStatus::Healthy, None)
                    .await?;
                self.set_health(cluster_id, HealthStatus::Healthy).await?;
            }
            "stop" => {
                provisioner.stop(&container).await?;
                self.set_cluster_status(cluster_id, ClusterStatus::Stopped, None)
                    .await?;
                self.set_health(cluster_id, HealthStatus::Unknown).await?;
            }
            "restart" => {
                self.set_cluster_status(cluster_id, ClusterStatus::Updating, None)
                    .await?;
                provisioner.restart(&container).await?;
                self.set_cluster_status(cluster_id, ClusterStatus::Healthy, None)
                    .await?;
                self.set_health(cluster_id, HealthStatus::Healthy).await?;
            }
            _ => return Err(Error::Job(format!("unknown action {action}"))),
        }
        Ok(serde_json::json!({"action": action, "ok": true}))
    }

    async fn handle_delete(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let mode: DeleteMode = serde_json::from_value(
            op.payload
                .get("mode")
                .cloned()
                .unwrap_or(serde_json::json!("remove_from_panel")),
        )
        .unwrap_or(DeleteMode::RemoveFromPanel);

        let row = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT docker_container_name, docker_network_name, docker_volume_name, delete_protection FROM clusters WHERE id = ?",
        )
        .bind(cluster_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("cluster".into()))?;

        let (container, network, volume, protection) = row;
        if protection != 0 && mode == DeleteMode::PermanentlyDelete {
            return Err(Error::DeleteProtection);
        }

        self.set_cluster_status(cluster_id, ClusterStatus::Deleting, None)
            .await?;

        let provisioner = self.provisioner_for_cluster(cluster_id).await?;

        match mode {
            DeleteMode::RemoveFromPanel => {
                self.queue
                    .append_log(op.id, "info", "removing from panel inventory only")
                    .await?;
            }
            DeleteMode::DeleteContainerKeepVolume => {
                provisioner
                    .deprovision(&container, &network, &volume, false)
                    .await?;
            }
            DeleteMode::PermanentlyDelete => {
                let confirm_vol = op
                    .payload
                    .get("confirm_volume_delete")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !confirm_vol {
                    return Err(Error::ConfirmationRequired(
                        "confirm_volume_delete required for permanent delete".into(),
                    ));
                }
                provisioner
                    .deprovision(&container, &network, &volume, true)
                    .await?;
            }
        }

        // Clean panel DB records
        sqlx::query("DELETE FROM cluster_credentials WHERE cluster_id = ?")
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM databases WHERE cluster_id = ?")
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM database_roles WHERE cluster_id = ?")
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM backup_integrations WHERE cluster_id = ?")
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM clusters WHERE id = ?")
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;

        Ok(serde_json::json!({"deleted": true, "mode": format!("{mode:?}")}))
    }

    async fn handle_enable_backup(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
        self.enable_backup_inner(cluster_id, &container).await?;
        self.set_cluster_status(cluster_id, ClusterStatus::Healthy, None)
            .await?;
        Ok(serde_json::json!({"backup_enabled": true, "engine": "native"}))
    }

    async fn enable_backup_inner(&self, cluster_id: Uuid, _container: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let schedule_id = Uuid::new_v4();
        let (retention_days, keep_count, _, schedule_hour) = self.backup_policy().await;
        let cron_default: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'backup.cron_default'")
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| format!("0 {schedule_hour} * * *"));
        let compression: i64 =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'backup.compression_level'")
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
                .and_then(|v: String| v.parse().ok())
                .unwrap_or(6);
        let verify_after = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'backup.verify_after'",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);

        sqlx::query(
            r#"
            INSERT INTO backup_schedules (
                id, cluster_id, cron, kind, database_name, enabled, retention_days, keep_count,
                compression_level, dump_format, schema_only, verify_after, notify_on_failure,
                created_at, updated_at
            )
            VALUES (?, ?, ?, 'logical_full', 'postgres', 1, ?, ?, ?, 'custom', 0, ?, 1, ?, ?)
            ON CONFLICT(cluster_id, database_name, kind) DO UPDATE SET
                enabled = 1,
                cron = excluded.cron,
                retention_days = excluded.retention_days,
                keep_count = excluded.keep_count,
                compression_level = excluded.compression_level,
                verify_after = excluded.verify_after,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(schedule_id.to_string())
        .bind(cluster_id.to_string())
        .bind(&cron_default)
        .bind(retention_days)
        .bind(keep_count)
        .bind(compression)
        .bind(if verify_after { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(format!("backup schedule: {e}")))?;

        sqlx::query(
            r#"
            INSERT INTO backup_integrations (id, cluster_id, status, external_id, message, manual_setup_info, failed_backups, created_at, updated_at)
            VALUES (?, ?, 'registered', 'native', 'Native PgPanel backup engine', NULL, 0, ?, ?)
            ON CONFLICT(cluster_id) DO UPDATE SET
                status = 'registered',
                external_id = 'native',
                message = 'Native PgPanel backup engine',
                updated_at = excluded.updated_at
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(cluster_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        sqlx::query(
            "UPDATE clusters SET databasus_status = ?, enable_backup = 1, updated_at = ? WHERE id = ?",
        )
        .bind(DatabasusIntegrationStatus::Registered.as_str())
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await
        .ok();

        let wal_default = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'backup.wal_archiving_default'",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
        if wal_default {
            sqlx::query(
                r#"
                INSERT INTO wal_streams (
                    cluster_id, enabled, archive_dir, compress, retention_days, status, created_at, updated_at
                ) VALUES (?, 1, '/var/lib/postgresql/wal_archive', 1, ?, 'configuring', ?, ?)
                ON CONFLICT(cluster_id) DO UPDATE SET
                    enabled = 1, retention_days = excluded.retention_days,
                    status = 'configuring', updated_at = excluded.updated_at
                "#,
            )
            .bind(cluster_id.to_string())
            .bind(retention_days)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(format!("WAL default setup: {e}")))?;
            let _ = self
                .queue
                .enqueue(
                    JobType::SyncWal,
                    Some(cluster_id),
                    serde_json::json!({}),
                    Some(&format!("wal-default-{cluster_id}")),
                )
                .await;
            sqlx::query(
                "UPDATE backup_integrations SET wal_status = 'configuring', updated_at = ? WHERE cluster_id = ?",
            )
            .bind(&now)
            .bind(cluster_id.to_string())
            .execute(&self.pool)
            .await
            .ok();
        }

        Ok(())
    }

    async fn handle_run_backup(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let database_spec = op
            .payload
            .get("database")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let schema_only = op
            .payload
            .get("schema_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let exclude_schemas: Vec<&str> = op
            .payload
            .get("exclude_schemas")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let exclude_tables: Vec<&str> = op
            .payload
            .get("exclude_tables")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let (_, _, encrypt, _) = self.backup_policy().await;

        let (username, password) = match self
            .load_credential_password(cluster_id, "pgpanel_backup")
            .await
        {
            Ok(pw) => ("pgpanel_backup".to_string(), pw),
            Err(_) => (
                "postgres".to_string(),
                self.load_admin_password(cluster_id).await?,
            ),
        };

        let databases = self
            .resolve_backup_databases(cluster_id, &container, &username, password.expose_secret(), &database_spec)
            .await?;
        if databases.is_empty() {
            return Err(Error::Job("no databases to back up".into()));
        }

        let mut results = Vec::new();
        let total = databases.len();
        for (idx, database) in databases.iter().enumerate() {
            let pct: u8 = (10 + ((idx as u32 * 80) / total.max(1) as u32)).min(99) as u8;
            self.queue
                .set_progress(
                    op.id,
                    pct,
                    &format!("backing up {database} ({}/{total})", idx + 1),
                )
                .await?;
            let one = self
                .run_one_logical_backup(
                    op.id,
                    cluster_id,
                    &container,
                    database,
                    &username,
                    password.expose_secret(),
                    schema_only,
                    encrypt,
                    &exclude_schemas,
                    &exclude_tables,
                )
                .await?;
            results.push(one);
        }

        self.queue
            .set_progress(op.id, 100, "backup complete")
            .await?;
        Ok(serde_json::json!({
            "databases": results,
            "count": results.len(),
            "scope": database_spec,
        }))
    }

    async fn resolve_backup_databases(
        &self,
        cluster_id: Uuid,
        container: &str,
        username: &str,
        password: &str,
        database_spec: &str,
    ) -> Result<Vec<String>> {
        let spec = database_spec.trim();
        if spec != "*" && !spec.is_empty() && spec != "__all__" {
            return Ok(vec![spec.to_string()]);
        }

        // Prefer live PostgreSQL catalog; fall back to panel registry + postgres.
        let listed = pgpanel_backup::dump::list_databases(container, username, password).await;
        if let Ok(dbs) = listed {
            let filtered: Vec<String> = dbs
                .into_iter()
                .filter(|d| d != "template0" && d != "template1")
                .collect();
            if !filtered.is_empty() {
                return Ok(filtered);
            }
        }

        let mut dbs: Vec<String> =
            sqlx::query_scalar("SELECT name FROM databases WHERE cluster_id = ? ORDER BY name")
                .bind(cluster_id.to_string())
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();
        if !dbs.iter().any(|d| d == "postgres") {
            dbs.insert(0, "postgres".into());
        }
        Ok(dbs)
    }

    async fn run_one_logical_backup(
        &self,
        op_id: Uuid,
        cluster_id: Uuid,
        container: &str,
        database: &str,
        username: &str,
        password: &str,
        schema_only: bool,
        encrypt: bool,
        exclude_schemas: &[&str],
        exclude_tables: &[&str],
    ) -> Result<serde_json::Value> {
        let backup_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO backups (id, cluster_id, kind, status, database_name, encrypted, started_at, created_at) VALUES (?, ?, ?, 'running', ?, ?, ?, ?)",
        )
        .bind(backup_id.to_string())
        .bind(cluster_id.to_string())
        .bind(if schema_only {
            "logical_schema"
        } else {
            "logical_full"
        })
        .bind(database)
        .bind(if encrypt { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let result = if exclude_schemas.is_empty() && exclude_tables.is_empty() {
            self.backup
                .run_logical_backup(RunLogicalBackupRequest {
                    cluster_id,
                    container_name: container.to_string(),
                    database: database.to_string(),
                    username: username.to_string(),
                    password: password.to_string(),
                    schema_only,
                    encrypt,
                })
                .await
        } else {
            use pgpanel_backup::dump::pg_dump_custom_ex;
            let raw = pg_dump_custom_ex(
                container,
                database,
                username,
                password,
                schema_only,
                exclude_schemas,
                exclude_tables,
                &[],
            )
            .await?;
            self.backup
                .store_raw_logical(cluster_id, database, schema_only, encrypt, &raw)
                .await
        };

        let finished = Utc::now().to_rfc3339();
        match result {
            Ok(r) => {
                sqlx::query(
                    "UPDATE backups SET status = 'succeeded', storage_key = ?, size_bytes = ?, checksum_sha256 = ?, finished_at = ? WHERE id = ?",
                )
                .bind(&r.storage_key)
                .bind(r.size_bytes as i64)
                .bind(&r.checksum_sha256)
                .bind(&finished)
                .bind(backup_id.to_string())
                .execute(&self.pool)
                .await
                .ok();

                sqlx::query(
                    "UPDATE backup_integrations SET last_successful_backup = ?, last_backup_status = 'succeeded', updated_at = ? WHERE cluster_id = ?",
                )
                .bind(&finished)
                .bind(&finished)
                .bind(cluster_id.to_string())
                .execute(&self.pool)
                .await
                .ok();

                let _ = op_id;
                Ok(serde_json::json!({
                    "backup_id": backup_id,
                    "database": database,
                    "storage_key": r.storage_key,
                    "size_bytes": r.size_bytes,
                    "checksum": r.checksum_sha256,
                }))
            }
            Err(e) => {
                sqlx::query(
                    "UPDATE backups SET status = 'failed', error = ?, finished_at = ? WHERE id = ?",
                )
                .bind(e.to_string())
                .bind(&finished)
                .bind(backup_id.to_string())
                .execute(&self.pool)
                .await
                .ok();
                sqlx::query(
                    "UPDATE backup_integrations SET last_backup_status = 'failed', failed_backups = failed_backups + 1, message = ?, updated_at = ? WHERE cluster_id = ?",
                )
                .bind(e.to_string())
                .bind(&finished)
                .bind(cluster_id.to_string())
                .execute(&self.pool)
                .await
                .ok();
                Err(Error::Backup(format!("{database}: {e}")))
            }
        }
    }

    async fn handle_restore_backup(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let backup_id = op
            .payload
            .get("backup_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("backup_id required".into()))?;
        let target_db = op
            .payload
            .get("target_database")
            .and_then(|v| v.as_str())
            .unwrap_or("postgres");
        let clean = op
            .payload
            .get("clean")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let row = sqlx::query_as::<_, (Option<String>, i64)>(
            "SELECT storage_key, encrypted FROM backups WHERE id = ? AND cluster_id = ?",
        )
        .bind(backup_id)
        .bind(cluster_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("backup".into()))?;

        let storage_key = row
            .0
            .ok_or_else(|| Error::NotFound("backup has no storage_key".into()))?;
        let encrypted = row.1 != 0;

        let (username, password) = match self.load_credential_password(cluster_id, "postgres").await
        {
            Ok(pw) => ("postgres".to_string(), pw),
            Err(_) => (
                "postgres".to_string(),
                self.load_admin_password(cluster_id).await?,
            ),
        };

        self.queue
            .set_progress(op.id, 30, "restoring logical dump")
            .await?;

        self.backup
            .restore_logical(
                &container,
                target_db,
                &username,
                password.expose_secret(),
                &storage_key,
                encrypted,
                clean,
            )
            .await?;

        self.queue
            .set_progress(op.id, 100, "restore complete")
            .await?;
        Ok(serde_json::json!({
            "backup_id": backup_id,
            "target_database": target_db,
            "clean": clean,
            "ok": true,
        }))
    }

    async fn handle_prune_backups(&self, op: &Operation) -> Result<serde_json::Value> {
        let (retention, keep_count, _, _) = self.backup_policy().await;
        let cluster_filter = op.cluster_id.map(|id| id.to_string());

        let rows: Vec<(String, Option<String>, String)> = if let Some(cid) = &cluster_filter {
            sqlx::query_as(
                r#"
                SELECT id, storage_key, created_at FROM backups
                WHERE cluster_id = ? AND status IN ('succeeded', 'verified')
                ORDER BY created_at DESC
                "#,
            )
            .bind(cid)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?
        } else {
            sqlx::query_as(
                r#"
                SELECT id, storage_key, created_at FROM backups
                WHERE status IN ('succeeded', 'verified')
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?
        };

        // Group by cluster via querying per-id is heavy; prune globally with keep_count + retention
        let cutoff = Utc::now() - chrono::Duration::days(retention);
        let mut deleted = 0u32;
        // Keep newest `keep_count` overall when no cluster filter — per cluster when filtered
        let keep = keep_count.max(1) as usize;
        for (idx, (id, key, created)) in rows.iter().enumerate() {
            let too_old = chrono::DateTime::parse_from_rfc3339(created)
                .map(|d| d.with_timezone(&Utc) < cutoff)
                .unwrap_or(false);
            let over_count = idx >= keep;
            if !(too_old || over_count) {
                continue;
            }
            // Always keep at least the newest one
            if idx == 0 {
                continue;
            }
            if let Some(k) = key {
                let _ = self.backup.delete_object(k).await;
            }
            let _ = sqlx::query("DELETE FROM backups WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await;
            deleted += 1;
        }

        Ok(
            serde_json::json!({ "deleted": deleted, "retention_days": retention, "keep_count": keep_count }),
        )
    }

    async fn handle_sync_wal(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let stream = sqlx::query_as::<_, (i64, String, i64)>(
            "SELECT enabled, archive_dir, retention_days FROM wal_streams WHERE cluster_id = ?",
        )
        .bind(cluster_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("WAL stream".into()))?;
        if stream.0 == 0 {
            return Ok(serde_json::json!({"enabled": false, "status": "disabled", "segments": 0}));
        }

        let _archive_dir = &stream.1;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
        let password = self.load_admin_password(cluster_id).await?;

        let mut status = match query_wal_status(&container, password.expose_secret()).await {
            Ok(status) => status,
            Err(error) => {
                self.set_wal_error(cluster_id, &error.to_string()).await;
                return Err(error);
            }
        };
        if status.archive_mode.as_deref() != Some("on") {
            self.queue
                .set_progress(op.id, 10, "applying PostgreSQL WAL archive settings")
                .await?;
            if let Err(error) = enable_archiving(&container, password.expose_secret()).await {
                self.set_wal_error(cluster_id, &error.to_string()).await;
                return Err(error);
            }
            let provisioner = self.provisioner_for_cluster(cluster_id).await?;
            if let Err(error) = provisioner.restart(&container).await {
                self.set_wal_error(cluster_id, &error.to_string()).await;
                return Err(error);
            }
            if let Err(error) = provisioner.wait_healthy(&container).await {
                self.set_wal_error(cluster_id, &error.to_string()).await;
                return Err(error);
            }
            status = match query_wal_status(&container, password.expose_secret()).await {
                Ok(status) => status,
                Err(error) => {
                    self.set_wal_error(cluster_id, &error.to_string()).await;
                    return Err(error);
                }
            };
        }
        if op
            .payload
            .get("switch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            switch_wal(&container, password.expose_secret()).await?;
        }

        let wal_root = self
            .config
            .backup_data_dir
            .clone()
            .unwrap_or_else(|| self.config.data_dir.join("backups"))
            .join("wal");
        let segments = match collect_segments(&container, &cluster_id.to_string(), &wal_root).await
        {
            Ok(segments) => segments,
            Err(error) => {
                self.set_wal_error(cluster_id, &error.to_string()).await;
                return Err(error);
            }
        };
        let local_cluster_root = wal_root.join(cluster_id.to_string());
        for segment in &segments {
            let local_path = local_cluster_root.join(&segment.filename);
            let bytes = tokio::fs::read(&local_path)
                .await
                .map_err(|e| Error::Job(format!("read WAL segment {}: {e}", segment.filename)))?;
            let storage_key = self
                .backup
                .store_wal_segment(&segment.storage_key, &bytes)
                .await?;
            sqlx::query(
                r#"
                INSERT INTO wal_segments (
                    id, cluster_id, filename, timeline, size_bytes, archived_at,
                    synced_at, storage_key, checksum_sha256
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(cluster_id, filename) DO UPDATE SET
                    timeline = excluded.timeline,
                    size_bytes = excluded.size_bytes,
                    archived_at = excluded.archived_at,
                    synced_at = excluded.synced_at,
                    storage_key = excluded.storage_key,
                    checksum_sha256 = excluded.checksum_sha256
                "#,
            )
            .bind(&segment.id)
            .bind(cluster_id.to_string())
            .bind(&segment.filename)
            .bind(segment.timeline as i64)
            .bind(segment.size_bytes as i64)
            .bind(&segment.archived_at)
            .bind(&segment.synced_at)
            .bind(&storage_key)
            .bind(&segment.checksum_sha256)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(format!("index WAL segment: {e}")))?;
        }

        let (count, total): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM wal_segments WHERE cluster_id = ?",
        )
        .bind(cluster_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        let last_segment: Option<String> = sqlx::query_scalar(
            "SELECT filename FROM wal_segments WHERE cluster_id = ? ORDER BY COALESCE(archived_at, synced_at) DESC LIMIT 1",
        )
        .bind(cluster_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        let stream_status = if status.failed_count > 0 {
            "degraded"
        } else {
            "active"
        };
        sqlx::query(
            r#"
            UPDATE wal_streams
            SET status = ?, last_segment = ?, last_synced_at = ?, last_error = NULL,
                timeline = COALESCE(?, timeline), segment_count = ?, total_bytes = ?, updated_at = ?
            WHERE cluster_id = ?
            "#,
        )
        .bind(stream_status)
        .bind(&last_segment)
        .bind(&now)
        .bind(
            status
                .last_archived_wal
                .as_deref()
                .and_then(|v| pgpanel_backup::parse_wal_filename(v).ok())
                .map(|v| v.timeline as i64),
        )
        .bind(count)
        .bind(total)
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        sqlx::query(
            "UPDATE backup_integrations SET wal_status = ?, updated_at = ? WHERE cluster_id = ?",
        )
        .bind(stream_status)
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await
        .ok();

        Ok(serde_json::json!({
            "enabled": true,
            "status": stream_status,
            "segments_synced": segments.len(),
            "segment_count": count,
            "total_bytes": total,
            "last_segment": last_segment,
            "archive_mode": status.archive_mode,
            "wal_level": status.wal_level,
            "retention_days": stream.2,
        }))
    }

    async fn handle_wal_base_backup(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
        let password = self.load_admin_password(cluster_id).await?;
        let (_, _, encrypt, _) = self.backup_policy().await;
        let backup_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO backups (id, cluster_id, kind, status, database_name, encrypted, started_at, created_at) VALUES (?, ?, 'base_backup', 'running', 'physical', ?, ?, ?)",
        )
        .bind(backup_id.to_string())
        .bind(cluster_id.to_string())
        .bind(if encrypt { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let result = match pg_basebackup_tar(&container, password.expose_secret()).await {
            Ok(bytes) => {
                self.backup
                    .store_base_backup(cluster_id, &bytes, encrypt)
                    .await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(stored) => {
                let finished = Utc::now().to_rfc3339();
                sqlx::query(
                    "UPDATE backups SET status = 'succeeded', storage_key = ?, size_bytes = ?, checksum_sha256 = ?, finished_at = ? WHERE id = ?",
                )
                .bind(&stored.storage_key)
                .bind(stored.size_bytes as i64)
                .bind(&stored.checksum_sha256)
                .bind(&finished)
                .bind(backup_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
                Ok(serde_json::json!({
                    "backup_id": backup_id,
                    "storage_key": stored.storage_key,
                    "size_bytes": stored.size_bytes,
                    "checksum": stored.checksum_sha256,
                }))
            }
            Err(error) => {
                let finished = Utc::now().to_rfc3339();
                sqlx::query(
                    "UPDATE backups SET status = 'failed', error = ?, finished_at = ? WHERE id = ?",
                )
                .bind(error.to_string())
                .bind(&finished)
                .bind(backup_id.to_string())
                .execute(&self.pool)
                .await
                .ok();
                Err(error)
            }
        }
    }

    async fn handle_wal_pitr_restore(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let target_time = op
            .payload
            .get("target_time")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Validation("target_time required".into()))?;
        let target = chrono::DateTime::parse_from_rfc3339(target_time)
            .map_err(|_| Error::Validation("target_time must be RFC3339".into()))?
            .with_timezone(&Utc);
        let base = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT id, storage_key, created_at, encrypted FROM backups WHERE cluster_id = ? AND kind = 'base_backup' AND status IN ('succeeded', 'verified') ORDER BY created_at DESC LIMIT 1",
        )
        .bind(cluster_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("no successful WAL base backup — take a base backup first".into()))?;
        if !self.backup.storage_exists(&base.1).await? {
            return Err(Error::NotFound("base backup object is unavailable".into()));
        }
        let wal_rows: Vec<(String, String, i64, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT filename, storage_key, size_bytes, archived_at, checksum_sha256 FROM wal_segments WHERE cluster_id = ? AND COALESCE(archived_at, synced_at) >= ? AND COALESCE(archived_at, synced_at) <= ? ORDER BY timeline, filename",
        )
        .bind(cluster_id.to_string())
        .bind(&base.2)
        .bind(target.to_rfc3339())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        if wal_rows.is_empty() {
            return Err(Error::Validation(
                "no WAL segments found between the latest base backup and the target time — sync WAL and try again".into(),
            ));
        }

        let restore_command =
            "cp /var/lib/postgresql/wal_archive/%f %p".to_string();
        let recovery_conf = format!(
            "# Generated by PgPanel PITR\n\
             restore_command = '{restore_command}'\n\
             recovery_target_time = '{target}'\n\
             recovery_target_action = 'promote'\n",
            target = target.to_rfc3339()
        );
        let auto_conf = format!(
            "# Generated by PgPanel PITR\n\
             restore_command = '{restore_command}'\n\
             recovery_target_time = '{target}'\n\
             recovery_target_action = 'promote'\n",
            target = target.to_rfc3339()
        );

        let manifest = serde_json::json!({
            "format": "pgpanel-pitr-manifest-v1",
            "cluster_id": cluster_id,
            "target_time": target.to_rfc3339(),
            "base_backup": {
                "id": base.0,
                "storage_key": base.1,
                "encrypted": base.3 != 0,
                "created_at": base.2,
            },
            "wal_segments": wal_rows.iter().map(|row| serde_json::json!({
                "filename": row.0,
                "storage_key": row.1,
                "size_bytes": row.2,
                "archived_at": row.3,
                "checksum_sha256": row.4,
            })).collect::<Vec<_>>(),
            "recovery_conf": recovery_conf,
            "postgresql_auto_conf": auto_conf,
            "steps": [
                "1. Extract the base backup tar into an empty PostgreSQL data directory",
                "2. Copy the listed WAL segment objects into wal_archive/",
                "3. Write recovery.signal (empty) and postgresql.auto.conf from this manifest",
                "4. Start PostgreSQL — it will recover to recovery_target_time then promote",
            ],
        });
        let key = format!(
            "pitr/{cluster_id}/recovery_{}.json",
            Utc::now().format("%Y%m%dT%H%M%SZ")
        );
        let storage_key = self
            .backup
            .store_manifest(&key, manifest.to_string().as_bytes())
            .await?;

        let backup_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO backups (
                id, cluster_id, kind, status, database_name, storage_key,
                size_bytes, checksum_sha256, encrypted, started_at, finished_at, created_at
            ) VALUES (?, ?, 'pitr_bundle', 'succeeded', 'postgres', ?, ?, NULL, 0, ?, ?, ?)
            "#,
        )
        .bind(backup_id.to_string())
        .bind(cluster_id.to_string())
        .bind(&storage_key)
        .bind(manifest.to_string().len() as i64)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        Ok(serde_json::json!({
            "manifest_storage_key": storage_key,
            "backup_id": backup_id,
            "target_time": target,
            "wal_segments": wal_rows.len(),
            "recoverable": true,
            "message": "PITR recovery package ready — download the manifest from backup history and follow the steps",
        }))
    }

    async fn handle_ping_node(&self, op: &Operation) -> Result<serde_json::Value> {
        let node_id = op
            .payload
            .get("node_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(NodeRegistry::local_node_id);
        let row = sqlx::query_as::<_, (String, Option<String>, i64)>(
            "SELECT kind, docker_host, docker_host_encrypted FROM nodes WHERE id = ?",
        )
        .bind(node_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("node".into()))?;

        let host = if row.0 == "local" {
            None
        } else if row.2 != 0 {
            let enc = row
                .1
                .ok_or_else(|| Error::Internal("missing docker_host".into()))?;
            let plain = decrypt_secret(&self.config.master_encryption_key, &enc)?;
            Some(plain.expose_secret().to_string())
        } else {
            row.1
        };

        self.nodes.ping_host(host.as_deref()).await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE nodes SET status = 'online', last_seen_at = ?, last_error = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(node_id.to_string())
        .execute(&self.pool)
        .await
        .ok();
        Ok(serde_json::json!({"ok": true, "node_id": node_id}))
    }

    async fn handle_duplicate_cluster(&self, op: &Operation) -> Result<serde_json::Value> {
        let primary_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing primary cluster_id".into()))?;
        let replica_row_id = op
            .payload
            .get("replica_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("replica_id required".into()))?;
        let target_node = op
            .payload
            .get("target_node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("target_node_id required".into()))?;
        let name = op
            .payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("replica")
            .to_string();

        let primary = sqlx::query_as::<_, (String, String, f64, i64, i64)>(
            "SELECT name, postgres_version, cpu_limit, memory_mb, storage_limit_gb FROM clusters WHERE id = ?",
        )
        .bind(primary_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let slug_base = format!(
            "{}_r_{}",
            pgpanel_core::validation::slugify(&primary.0),
            &Uuid::new_v4().to_string()[..8]
        );
        let now = Utc::now().to_rfc3339();
        let new_id = Uuid::new_v4();
        let names = self.provisioner.resource_names(&slug_base);
        let admin_password = generate_password();
        let enc = encrypt_secret(&self.config.master_encryption_key, &admin_password)?;

        sqlx::query(
            r#"
            INSERT INTO clusters (
                id, name, slug, postgres_version, docker_container_id, docker_container_name,
                docker_volume_name, docker_network_name, internal_hostname, public_port,
                cpu_limit, memory_mb, storage_limit_gb, status, health, databasus_status,
                delete_protection, enable_backup, node_id, environment, description, created_at, updated_at
            ) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, NULL, ?, ?, ?, 'creating', 'unknown',
                      'not_configured', 0, 0, ?, 'replica', ?, ?, ?)
            "#,
        )
        .bind(new_id.to_string())
        .bind(format!("{} (replica)", name))
        .bind(&slug_base)
        .bind(&primary.1)
        .bind(&names.container_name)
        .bind(&names.volume_name)
        .bind(&names.network_name)
        .bind(&names.internal_hostname)
        .bind(primary.2)
        .bind(primary.3)
        .bind(primary.4)
        .bind(target_node)
        .bind(format!("Replica of {primary_id}"))
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO cluster_credentials (id, cluster_id, role_name, username, password_encrypted, kind, created_at, updated_at)
            VALUES (?, ?, 'postgres', 'postgres', ?, 'admin', ?, ?)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(new_id.to_string())
        .bind(&enc)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let req = CreateClusterRequest {
            name: format!("{} (replica)", name),
            postgres_version: primary.1.clone(),
            cpu_limit: primary.2,
            memory_mb: primary.3 as u32,
            storage_limit_gb: primary.4 as u32,
            expose_publicly: false,
            optional_public_port: None,
            enable_backup: false,
            node_id: Uuid::parse_str(target_node).ok(),
            initial_databases: vec![],
        };

        sqlx::query(
            "UPDATE cluster_replicas SET status = 'syncing', replica_cluster_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(new_id.to_string())
        .bind(&now)
        .bind(replica_row_id)
        .execute(&self.pool)
        .await
        .ok();

        let provisioner = self.provisioner_for_cluster(new_id).await?;
        match provisioner
            .provision(new_id, &slug_base, &req, admin_password.expose_secret())
            .await
        {
            Ok(_) => {
                self.set_cluster_status(new_id, ClusterStatus::Healthy, None)
                    .await?;
                // Initial data sync from primary backup dump
                let _ = self
                    .queue
                    .enqueue(
                        JobType::SyncReplica,
                        Some(primary_id),
                        serde_json::json!({"replica_id": replica_row_id}),
                        None,
                    )
                    .await;
                sqlx::query(
                    "UPDATE cluster_replicas SET status = 'healthy', updated_at = ? WHERE id = ?",
                )
                .bind(&now)
                .bind(replica_row_id)
                .execute(&self.pool)
                .await
                .ok();
                Ok(serde_json::json!({
                    "replica_cluster_id": new_id,
                    "status": "healthy",
                }))
            }
            Err(e) => {
                sqlx::query(
                    "UPDATE cluster_replicas SET status = 'failed', last_error = ?, updated_at = ? WHERE id = ?",
                )
                .bind(e.to_string())
                .bind(&now)
                .bind(replica_row_id)
                .execute(&self.pool)
                .await
                .ok();
                Err(e)
            }
        }
    }

    async fn handle_sync_replica(&self, op: &Operation) -> Result<serde_json::Value> {
        let replica_id = op
            .payload
            .get("replica_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("replica_id required".into()))?;
        let row = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT primary_cluster_id, replica_cluster_id FROM cluster_replicas WHERE id = ?",
        )
        .bind(replica_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("replica".into()))?;

        let primary_id = Uuid::parse_str(&row.0).map_err(|e| Error::Job(e.to_string()))?;
        let replica_cluster = row
            .1
            .ok_or_else(|| Error::Job("replica cluster not provisioned yet".into()))?;
        let replica_uuid =
            Uuid::parse_str(&replica_cluster).map_err(|e| Error::Job(e.to_string()))?;

        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE cluster_replicas SET status = 'syncing', updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(replica_id)
            .execute(&self.pool)
            .await
            .ok();

        // Dump from primary
        let primary_container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(primary_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;
        let replica_container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(replica_uuid.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let pw = self.load_admin_password(primary_id).await?;
        let dump = pgpanel_backup::dump::pg_dump_custom(
            &primary_container,
            "postgres",
            "postgres",
            pw.expose_secret(),
            false,
        )
        .await?;

        let replica_pw = self.load_admin_password(replica_uuid).await?;
        pgpanel_backup::dump::pg_restore_custom(
            &replica_container,
            "postgres",
            "postgres",
            replica_pw.expose_secret(),
            &dump,
            true,
        )
        .await?;

        let finished = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE cluster_replicas SET status = 'healthy', last_sync_at = ?, lag_seconds = 0, last_error = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(&finished)
        .bind(&finished)
        .bind(replica_id)
        .execute(&self.pool)
        .await
        .ok();

        Ok(serde_json::json!({"ok": true, "synced_at": finished}))
    }

    async fn handle_promote_replica(&self, op: &Operation) -> Result<serde_json::Value> {
        let replica_id = op
            .payload
            .get("replica_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("replica_id required".into()))?;
        let row = sqlx::query_as::<_, (String, Option<String>, i64)>(
            "SELECT primary_cluster_id, replica_cluster_id, promote_protection FROM cluster_replicas WHERE id = ?",
        )
        .bind(replica_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?
        .ok_or_else(|| Error::NotFound("replica".into()))?;

        if row.2 != 0
            && !op
                .payload
                .get("confirm")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            return Err(Error::ConfirmationRequired(
                "confirm=true required to promote replica".into(),
            ));
        }
        let replica_cluster = row
            .1
            .ok_or_else(|| Error::Job("no replica cluster".into()))?;
        let now = Utc::now().to_rfc3339();
        // Mark former primary as standby metadata; replica becomes independent primary
        sqlx::query(
            "UPDATE clusters SET environment = 'production', description = 'Promoted from replica', updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(&replica_cluster)
        .execute(&self.pool)
        .await
        .ok();
        sqlx::query(
            "UPDATE clusters SET environment = 'former_primary', updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(&row.0)
        .execute(&self.pool)
        .await
        .ok();
        sqlx::query(
            "UPDATE cluster_replicas SET status = 'promoted', enabled = 0, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(replica_id)
        .execute(&self.pool)
        .await
        .ok();
        Ok(serde_json::json!({
            "promoted_cluster_id": replica_cluster,
            "former_primary_id": row.0,
        }))
    }

    async fn handle_evaluate_alerts(&self, _op: &Operation) -> Result<serde_json::Value> {
        let rules: Vec<(String, String, String, String, f64, String, Option<String>)> =
            sqlx::query_as(
                r#"
                SELECT id, name, severity, metric, threshold, operator, scope_id
                FROM alert_rules WHERE enabled = 1
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let mut fired = 0u32;
        for (id, name, severity, metric, threshold, operator, _scope) in rules {
            let value = match metric.as_str() {
                "cpu_percent" => sqlx::query_scalar::<_, f64>(
                    "SELECT COALESCE(AVG(cpu_percent), 0) FROM cluster_metrics WHERE collected_at > datetime('now', '-10 minutes')",
                )
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0.0),
                "memory_percent" => sqlx::query_scalar::<_, f64>(
                    "SELECT COALESCE(AVG(CASE WHEN memory_limit_mb > 0 THEN 100.0 * memory_usage_mb / memory_limit_mb ELSE 0 END), 0) FROM cluster_metrics WHERE collected_at > datetime('now', '-10 minutes')",
                )
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0.0),
                "backup_age_hours" => {
                    let last: Option<String> = sqlx::query_scalar(
                        "SELECT MAX(last_successful_backup) FROM backup_integrations",
                    )
                    .fetch_optional(&self.pool)
                    .await
                    .ok()
                    .flatten()
                    .flatten();
                    match last.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok()) {
                        Some(dt) => {
                            (Utc::now() - dt.with_timezone(&Utc)).num_seconds() as f64 / 3600.0
                        }
                        None => 9999.0,
                    }
                }
                "replica_lag" => sqlx::query_scalar::<_, f64>(
                    "SELECT COALESCE(MAX(lag_seconds), 0) FROM cluster_replicas WHERE enabled = 1 AND status != 'paused'",
                )
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0.0) as f64,
                "failed_backups" => sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM backups WHERE status = 'failed' AND created_at > datetime('now', '-24 hours')",
                )
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0) as f64,
                _ => continue,
            };

            let hit = match operator.as_str() {
                "gte" => value >= threshold,
                "lt" => value < threshold,
                "lte" => value <= threshold,
                "eq" => (value - threshold).abs() < f64::EPSILON,
                _ => value > threshold,
            };
            if !hit {
                continue;
            }
            // cooldown: skip if open alert for same rule in last hour
            let recent: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM alerts WHERE rule_id = ? AND status = 'open' AND fired_at > datetime('now', '-1 hour')",
            )
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
            if recent > 0 {
                continue;
            }
            let alert_id = Uuid::new_v4();
            let now = Utc::now().to_rfc3339();
            sqlx::query(
                r#"
                INSERT INTO alerts (id, rule_id, severity, title, message, resource_type, status, fired_at, details)
                VALUES (?, ?, ?, ?, ?, 'panel', 'open', ?, ?)
                "#,
            )
            .bind(alert_id.to_string())
            .bind(&id)
            .bind(&severity)
            .bind(&name)
            .bind(format!("{name}: {metric}={value:.2} (threshold {threshold})"))
            .bind(&now)
            .bind(serde_json::json!({"metric": metric, "value": value}).to_string())
            .execute(&self.pool)
            .await
            .ok();
            fired += 1;
            if let Ok(url) = std::env::var("WEBHOOK_URL") {
                if !url.is_empty() {
                    pgpanel_backup::notify_webhook(
                        &url,
                        &format!("PgPanel alert: {name}"),
                        &format!("{metric}={value:.2}"),
                    )
                    .await;
                }
            }
        }
        Ok(serde_json::json!({"fired": fired}))
    }

    async fn handle_verify_backup(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let storage_key: String = if let Some(k) =
            op.payload.get("storage_key").and_then(|v| v.as_str())
        {
            k.to_string()
        } else {
            sqlx::query_scalar(
                "SELECT storage_key FROM backups WHERE cluster_id = ? AND status = 'succeeded' ORDER BY created_at DESC LIMIT 1",
            )
            .bind(cluster_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?
            .flatten()
            .ok_or_else(|| Error::NotFound("no successful backup".into()))?
        };

        let listing = self
            .backup
            .verify_logical(&container, &storage_key, storage_key.ends_with(".enc"))
            .await?;
        Ok(serde_json::json!({
            "storage_key": storage_key,
            "pg_restore_list": listing.chars().take(4000).collect::<String>(),
            "ok": true,
        }))
    }

    async fn backup_policy(&self) -> (i64, i64, bool, i64) {
        let get = |key: &'static str| async move {
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten()
        };
        let retention = get("backup.retention_days")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.config.backup_retention_days as i64);
        let keep_count = get("backup.keep_count")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.config.backup_keep_count as i64);
        let encrypt = get("backup.encrypt")
            .await
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(self.config.backup_encrypt);
        let schedule_hour = get("backup.schedule_hour")
            .await
            .and_then(|v| v.parse().ok())
            .filter(|hour: &i64| (0..=23).contains(hour))
            .unwrap_or(3);
        (retention, keep_count, encrypt, schedule_hour)
    }

    async fn handle_refresh_metrics(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let container: String =
            sqlx::query_scalar("SELECT docker_container_name FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let stats = self
            .provisioner
            .docker()
            .container_stats(&container)
            .await
            .map_err(|e| Error::Docker(e.to_string()))?;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO cluster_metrics (cluster_id, cpu_percent, memory_usage_mb, memory_limit_mb, collected_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(cluster_id.to_string())
        .bind(stats.cpu_percent)
        .bind(stats.memory_usage_mb)
        .bind(stats.memory_limit_mb)
        .bind(&now)
        .execute(&self.pool)
        .await
        .ok();

        Ok(serde_json::json!({
            "cpu_percent": stats.cpu_percent,
            "memory_usage_mb": stats.memory_usage_mb,
            "memory_limit_mb": stats.memory_limit_mb,
            "collected_at": now,
        }))
    }

    async fn handle_create_database(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let req: CreateDatabaseRequest = serde_json::from_value(op.payload.clone())
            .map_err(|e| Error::Job(format!("payload: {e}")))?;

        let (host, slug): (String, String) = sqlx::query_as(
            "SELECT internal_hostname, slug FROM clusters WHERE id = ?",
        )
        .bind(cluster_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let role_name = req
            .role_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| slug.clone());

        let client = self.connect_cluster_admin(cluster_id, &host).await?;
        let roles = RoleService::new(&client);

        let role_existed = roles.role_exists(&role_name).await?;
        let mut created_password: Option<String> = None;

        if !role_existed {
            let password = if req.generate_password || req.password.is_none() {
                generate_password()
            } else {
                let p = req.password.clone().unwrap_or_default();
                pgpanel_core::crypto::validate_password_strength(&p)?;
                SecretString::from(p)
            };
            roles
                .create_role(&role_name, &password, req.connection_limit)
                .await?;
            let enc = encrypt_secret(&self.config.master_encryption_key, &password)?;
            self.store_credential(cluster_id, &role_name, "app", &enc)
                .await?;
            created_password = Some(password.expose_secret().to_string());
        }

        if !roles.database_exists(&req.database_name).await? {
            match roles
                .create_database(&req.database_name, &role_name, req.connection_limit)
                .await
            {
                Ok(()) => {}
                Err(e) => {
                    self.queue
                        .append_log(
                            op.id,
                            "error",
                            &format!(
                                "database creation failed after role created; retry possible: {e}"
                            ),
                        )
                        .await?;
                    return Err(e);
                }
            }
        }

        // Cluster primary user (slug) always gets CONNECT on every app DB.
        if role_name != slug && roles.role_exists(&slug).await? {
            let mut allowed = roles.list_connectable_databases(&slug).await.unwrap_or_default();
            if !allowed.iter().any(|d| d == &req.database_name) {
                allowed.push(req.database_name.clone());
            }
            if let Err(e) = roles.set_database_access(&slug, &allowed).await {
                self.queue
                    .append_log(
                        op.id,
                        "warn",
                        &format!("grant cluster user {slug} on {}: {e}", req.database_name),
                    )
                    .await?;
            }
        }

        let now = Utc::now().to_rfc3339();
        let db_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();

        sqlx::query(
            "INSERT OR IGNORE INTO databases (id, cluster_id, name, owner_role, connection_limit, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(db_id.to_string())
        .bind(cluster_id.to_string())
        .bind(&req.database_name)
        .bind(&role_name)
        .bind(req.connection_limit)
        .bind(&now)
        .execute(&self.pool)
        .await
        .ok();

        sqlx::query(
            "INSERT OR IGNORE INTO database_roles (id, cluster_id, name, is_superuser, can_login, connection_limit, created_at) VALUES (?, ?, ?, 0, 1, ?, ?)",
        )
        .bind(role_id.to_string())
        .bind(cluster_id.to_string())
        .bind(&role_name)
        .bind(req.connection_limit)
        .bind(&now)
        .execute(&self.pool)
        .await
        .ok();

        Ok(serde_json::json!({
            "database_id": db_id,
            "role_id": role_id,
            "database_name": req.database_name,
            "role_name": role_name,
            "password": created_password,
            "connection_string": created_password.as_ref().map(|pw| format!(
                "postgresql://{}:{}@{}:5432/{}",
                role_name,
                urlencoding_minimal(pw),
                host,
                req.database_name
            )),
        }))
    }

    async fn handle_update_networking(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let expose = op
            .payload
            .get("expose_publicly")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let public_port = op
            .payload
            .get("public_port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16);
        let public_port = if expose { public_port } else { None };

        #[derive(sqlx::FromRow)]
        struct Row {
            slug: String,
            postgres_version: String,
            cpu_limit: f64,
            memory_mb: i64,
            docker_container_name: String,
        }

        let row = sqlx::query_as::<_, Row>(
            "SELECT slug, postgres_version, cpu_limit, memory_mb, docker_container_name FROM clusters WHERE id = ?",
        )
        .bind(cluster_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        self.set_cluster_status(cluster_id, ClusterStatus::Updating, None)
            .await?;
        self.queue
            .set_progress(op.id, 20, "recreating container with new port mapping")
            .await?;

        let admin = self.load_admin_password(cluster_id).await?;
        let provisioner = self.provisioner_for_cluster(cluster_id).await?;
        let container_id = provisioner
            .recreate_with_public_port(
                cluster_id,
                &row.slug,
                &row.postgres_version,
                row.cpu_limit,
                row.memory_mb as u32,
                admin.expose_secret(),
                public_port,
            )
            .await?;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE clusters SET public_port = ?, docker_container_id = ?, status = ?, health = ?, updated_at = ? WHERE id = ?",
        )
        .bind(public_port.map(|p| p as i64))
        .bind(&container_id)
        .bind(ClusterStatus::Healthy.as_str())
        .bind(HealthStatus::Healthy.as_str())
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        Ok(serde_json::json!({
            "cluster_id": cluster_id,
            "container_id": container_id,
            "container_name": row.docker_container_name,
            "public_port": public_port,
            "expose_publicly": expose,
        }))
    }

    async fn handle_delete_database(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let db_name = op
            .payload
            .get("database_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("database_name required".into()))?;

        let host: String =
            sqlx::query_scalar("SELECT internal_hostname FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let client = self.connect_cluster_admin(cluster_id, &host).await?;
        RoleService::new(&client).drop_database(db_name).await?;

        sqlx::query("DELETE FROM databases WHERE cluster_id = ? AND name = ?")
            .bind(cluster_id.to_string())
            .bind(db_name)
            .execute(&self.pool)
            .await
            .ok();

        Ok(serde_json::json!({"deleted": db_name}))
    }

    async fn handle_rotate_password(&self, op: &Operation) -> Result<serde_json::Value> {
        let cluster_id = op
            .cluster_id
            .ok_or_else(|| Error::Job("missing cluster_id".into()))?;
        let role_name = op
            .payload
            .get("role_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Job("role_name required".into()))?
            .to_string();

        let generate = op
            .payload
            .get("generate_password")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let password = if generate {
            generate_password()
        } else {
            let p = op
                .payload
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Validation("password required".into()))?;
            pgpanel_core::crypto::validate_password_strength(p)?;
            SecretString::from(p.to_string())
        };

        let host: String =
            sqlx::query_scalar("SELECT internal_hostname FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let client = self.connect_cluster_admin(cluster_id, &host).await?;
        RoleService::new(&client)
            .change_password(&role_name, &password)
            .await?;

        let enc = encrypt_secret(&self.config.master_encryption_key, &password)?;
        self.store_credential(cluster_id, &role_name, "app", &enc)
            .await?;

        Ok(serde_json::json!({
            "role_name": role_name,
            "password": password.expose_secret(),
            "warning": "Update application connection strings; the previous password is no longer valid."
        }))
    }

    // ── helpers ──────────────────────────────────────────────────────────

    async fn set_cluster_status(
        &self,
        id: Uuid,
        status: ClusterStatus,
        err: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE clusters SET status = ?, last_error = ?, updated_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(err)
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;
        Ok(())
    }

    async fn set_wal_error(&self, cluster_id: Uuid, message: &str) {
        let now = Utc::now().to_rfc3339();
        let _ = sqlx::query(
            "UPDATE wal_streams SET status = 'error', last_error = ?, updated_at = ? WHERE cluster_id = ?",
        )
        .bind(message)
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await;
        let _ = sqlx::query(
            "UPDATE backup_integrations SET wal_status = 'error', message = ?, updated_at = ? WHERE cluster_id = ?",
        )
        .bind(message)
        .bind(&now)
        .bind(cluster_id.to_string())
        .execute(&self.pool)
        .await;
    }

    async fn set_health(&self, id: Uuid, health: HealthStatus) -> Result<()> {
        sqlx::query("UPDATE clusters SET health = ?, updated_at = ? WHERE id = ?")
            .bind(health.as_str())
            .bind(Utc::now().to_rfc3339())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Job(e.to_string()))?;
        Ok(())
    }

    async fn load_admin_password(&self, cluster_id: Uuid) -> Result<SecretString> {
        let enc: String = sqlx::query_scalar(
            "SELECT password_encrypted FROM cluster_credentials WHERE cluster_id = ? AND role_name = 'postgres'",
        )
        .bind(cluster_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Job(format!("admin credential: {e}")))?;
        decrypt_secret(&self.config.master_encryption_key, &enc)
    }

    async fn load_credential_password(&self, cluster_id: Uuid, role: &str) -> Result<SecretString> {
        let enc: String = sqlx::query_scalar(
            "SELECT password_encrypted FROM cluster_credentials WHERE cluster_id = ? AND role_name = ?",
        )
        .bind(cluster_id.to_string())
        .bind(role)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::NotFound(format!("credential {role}: {e}")))?;
        decrypt_secret(&self.config.master_encryption_key, &enc)
    }

    async fn store_credential(
        &self,
        cluster_id: Uuid,
        role: &str,
        kind: &str,
        encrypted: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO cluster_credentials (id, cluster_id, role_name, username, password_encrypted, kind, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(cluster_id, role_name) DO UPDATE SET
                password_encrypted = excluded.password_encrypted,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(cluster_id.to_string())
        .bind(role)
        .bind(role)
        .bind(encrypted)
        .bind(kind)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;
        Ok(())
    }

    async fn connect_cluster_admin(&self, cluster_id: Uuid, host: &str) -> Result<PgClient> {
        let password = self.load_admin_password(cluster_id).await?;
        // Inside Docker network, use internal hostname:5432.
        // For host-side dev, allow override via public port.
        let public_port: Option<i64> =
            sqlx::query_scalar("SELECT public_port FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .ok()
                .flatten();

        // Try internal host first, then localhost with public port
        match PgClient::connect(
            host,
            5432,
            "postgres",
            &password,
            "postgres",
            self.config.default_statement_timeout_ms,
            self.config.default_lock_timeout_ms,
        )
        .await
        {
            Ok(c) => Ok(c),
            Err(e1) => {
                if let Some(port) = public_port {
                    info!(%host, error = %e1, "internal connect failed; trying localhost");
                    PgClient::connect(
                        "127.0.0.1",
                        port as u16,
                        "postgres",
                        &password,
                        "postgres",
                        self.config.default_statement_timeout_ms,
                        self.config.default_lock_timeout_ms,
                    )
                    .await
                } else {
                    // Last resort: container name via docker bridge is still host
                    Err(e1)
                }
            }
        }
    }
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}
