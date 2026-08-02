use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use pgpanel_core::config::Config;
use pgpanel_core::crypto::{decrypt_secret, encrypt_secret, generate_password};
use pgpanel_core::error::{Error, Result};
use pgpanel_backup::{BackupEngine, RunLogicalBackupRequest};
use pgpanel_core::models::{
    ClusterStatus, CreateClusterRequest, CreateDatabaseRequest, DatabasusIntegrationStatus,
    DeleteMode, HealthStatus, JobType, Operation,
};
use pgpanel_docker::ClusterProvisioner;
use pgpanel_postgres::{PgClient, RoleService};

use crate::queue::JobQueue;

pub struct JobContext {
    pub queue: JobQueue,
    pub pool: SqlitePool,
    pub config: Config,
    pub provisioner: ClusterProvisioner,
    pub backup: Arc<BackupEngine>,
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
            JobType::CreateDatabase => self.handle_create_database(&op).await,
            JobType::DeleteDatabase => self.handle_delete_database(&op).await,
            JobType::RotatePassword => self.handle_rotate_password(&op).await,
            JobType::RefreshMetrics => self.handle_refresh_metrics(&op).await,
        }
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
            .provisioner
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
            for spec in &req.initial_databases {
                self.queue
                    .append_log(
                        op.id,
                        "info",
                        &format!(
                            "creating initial database {} owner {}",
                            spec.database_name, spec.role_name
                        ),
                    )
                    .await?;
                let password = if let Some(ref p) = spec.password {
                    if let Err(e) = pgpanel_core::crypto::validate_password_strength(p) {
                        self.queue
                            .append_log(op.id, "warn", &format!("password for {}: {e}", spec.role_name))
                            .await?;
                        generate_password()
                    } else {
                        SecretString::from(p.clone())
                    }
                } else {
                    generate_password()
                };

                if let Err(e) = roles.create_role(&spec.role_name, &password, None).await {
                    self.queue
                        .append_log(op.id, "error", &format!("create role {}: {e}", spec.role_name))
                        .await?;
                    continue;
                }
                if let Err(e) = roles
                    .create_database(&spec.database_name, &spec.role_name, None)
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
                self.store_credential(cluster_id, &spec.role_name, "app", &enc)
                    .await?;

                let now = Utc::now().to_rfc3339();
                let db_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO databases (id, cluster_id, name, owner_role, connection_limit, created_at) VALUES (?, ?, ?, ?, NULL, ?)",
                )
                .bind(db_id.to_string())
                .bind(cluster_id.to_string())
                .bind(&spec.database_name)
                .bind(&spec.role_name)
                .bind(&now)
                .execute(&self.pool)
                .await;

                let role_id = Uuid::new_v4();
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO database_roles (id, cluster_id, name, is_superuser, can_login, connection_limit, created_at) VALUES (?, ?, ?, 0, 1, NULL, ?)",
                )
                .bind(role_id.to_string())
                .bind(cluster_id.to_string())
                .bind(&spec.role_name)
                .bind(&now)
                .execute(&self.pool)
                .await;

                created_roles.push(serde_json::json!({
                    "database": spec.database_name,
                    "role": spec.role_name,
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
            if let Err(e) = self.enable_backup_inner(cluster_id, &names.container_name).await {
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

        match action {
            "start" => {
                self.set_cluster_status(cluster_id, ClusterStatus::Starting, None)
                    .await?;
                self.provisioner.start(&container).await?;
                self.set_cluster_status(cluster_id, ClusterStatus::Healthy, None)
                    .await?;
                self.set_health(cluster_id, HealthStatus::Healthy).await?;
            }
            "stop" => {
                self.provisioner.stop(&container).await?;
                self.set_cluster_status(cluster_id, ClusterStatus::Stopped, None)
                    .await?;
                self.set_health(cluster_id, HealthStatus::Unknown).await?;
            }
            "restart" => {
                self.set_cluster_status(cluster_id, ClusterStatus::Updating, None)
                    .await?;
                self.provisioner.restart(&container).await?;
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

        match mode {
            DeleteMode::RemoveFromPanel => {
                self.queue
                    .append_log(op.id, "info", "removing from panel inventory only")
                    .await?;
            }
            DeleteMode::DeleteContainerKeepVolume => {
                self.provisioner
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
                self.provisioner
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
        sqlx::query(
            r#"
            INSERT INTO backup_schedules (id, cluster_id, cron, kind, database_name, enabled, retention_days, keep_count, created_at, updated_at)
            VALUES (?, ?, '0 3 * * *', 'logical_full', 'postgres', 1, ?, ?, ?, ?)
            ON CONFLICT(cluster_id, database_name, kind) DO UPDATE SET
                enabled = 1,
                retention_days = excluded.retention_days,
                keep_count = excluded.keep_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(schedule_id.to_string())
        .bind(cluster_id.to_string())
        .bind(self.config.backup_retention_days as i64)
        .bind(self.config.backup_keep_count as i64)
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

        let database = op
            .payload
            .get("database")
            .and_then(|v| v.as_str())
            .unwrap_or("postgres")
            .to_string();
        let schema_only = op
            .payload
            .get("schema_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

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

        self.queue
            .set_progress(op.id, 20, "running pg_dump")
            .await?;

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
        .bind(&database)
        .bind(if self.config.backup_encrypt { 1 } else { 0 })
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Job(e.to_string()))?;

        let result = self
            .backup
            .run_logical_backup(RunLogicalBackupRequest {
                cluster_id,
                container_name: container,
                database: database.clone(),
                username,
                password: password.expose_secret().to_string(),
                schema_only,
                encrypt: self.config.backup_encrypt,
            })
            .await;

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

                self.queue
                    .set_progress(op.id, 100, "backup complete")
                    .await?;
                Ok(serde_json::json!({
                    "backup_id": backup_id,
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
                Err(Error::Backup(e.to_string()))
            }
        }
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

        let storage_key: String = if let Some(k) = op.payload.get("storage_key").and_then(|v| v.as_str()) {
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

        let host: String =
            sqlx::query_scalar("SELECT internal_hostname FROM clusters WHERE id = ?")
                .bind(cluster_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| Error::Job(e.to_string()))?;

        let client = self.connect_cluster_admin(cluster_id, &host).await?;
        let roles = RoleService::new(&client);

        let password = if req.generate_password || req.password.is_none() {
            generate_password()
        } else {
            let p = req.password.clone().unwrap_or_default();
            pgpanel_core::crypto::validate_password_strength(&p)?;
            SecretString::from(p)
        };

        // Partial failure handling: role may exist without database
        if !roles.role_exists(&req.role_name).await? {
            roles
                .create_role(&req.role_name, &password, req.connection_limit)
                .await?;
        } else {
            // Role exists (retry) — update password
            roles.change_password(&req.role_name, &password).await?;
        }

        if !roles.database_exists(&req.database_name).await? {
            match roles
                .create_database(&req.database_name, &req.role_name, req.connection_limit)
                .await
            {
                Ok(()) => {}
                Err(e) => {
                    // Leave role in place for retry; record partial state
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

        let now = Utc::now().to_rfc3339();
        let db_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();

        sqlx::query(
            "INSERT OR IGNORE INTO databases (id, cluster_id, name, owner_role, connection_limit, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(db_id.to_string())
        .bind(cluster_id.to_string())
        .bind(&req.database_name)
        .bind(&req.role_name)
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
        .bind(&req.role_name)
        .bind(req.connection_limit)
        .bind(&now)
        .execute(&self.pool)
        .await
        .ok();

        let enc = encrypt_secret(&self.config.master_encryption_key, &password)?;
        self.store_credential(cluster_id, &req.role_name, "app", &enc)
            .await?;

        // Password returned only via operation result once (API layer exposes once)
        Ok(serde_json::json!({
            "database_id": db_id,
            "role_id": role_id,
            "database_name": req.database_name,
            "role_name": req.role_name,
            "password": password.expose_secret(),
            "connection_string": format!(
                "postgresql://{}:{}@{}:5432/{}",
                req.role_name,
                urlencoding_minimal(password.expose_secret()),
                host,
                req.database_name
            )
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
