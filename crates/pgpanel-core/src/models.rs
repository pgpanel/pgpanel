//! Domain models shared across crates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ── Cluster ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClusterStatus {
    Creating,
    Starting,
    Healthy,
    Degraded,
    HealthyWithBackupWarning,
    Stopped,
    Updating,
    Deleting,
    Failed,
}

impl ClusterStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Starting => "starting",
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::HealthyWithBackupWarning => "healthy_with_backup_warning",
            Self::Stopped => "stopped",
            Self::Updating => "updating",
            Self::Deleting => "deleting",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "creating" => Some(Self::Creating),
            "starting" => Some(Self::Starting),
            "healthy" => Some(Self::Healthy),
            "degraded" => Some(Self::Degraded),
            "healthy_with_backup_warning" => Some(Self::HealthyWithBackupWarning),
            "stopped" => Some(Self::Stopped),
            "updating" => Some(Self::Updating),
            "deleting" => Some(Self::Deleting),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::Healthy | Self::Degraded | Self::HealthyWithBackupWarning | Self::Starting
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Unhealthy,
    Starting,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Unhealthy => "unhealthy",
            Self::Starting => "starting",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "healthy" => Self::Healthy,
            "unhealthy" => Self::Unhealthy,
            "starting" => Self::Starting,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatabasusIntegrationStatus {
    NotConfigured,
    Pending,
    PendingManualSetup,
    Registered,
    Error,
}

impl DatabasusIntegrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Pending => "pending",
            Self::PendingManualSetup => "pending_manual_setup",
            Self::Registered => "registered",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "pending_manual_setup" => Self::PendingManualSetup,
            "registered" => Self::Registered,
            "error" => Self::Error,
            _ => Self::NotConfigured,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Cluster {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub postgres_version: String,
    pub docker_container_id: Option<String>,
    pub docker_container_name: String,
    pub docker_volume_name: String,
    pub docker_network_name: String,
    pub internal_hostname: String,
    pub public_port: Option<u16>,
    pub cpu_limit: f64,
    pub memory_mb: u32,
    pub storage_limit_gb: u32,
    pub status: ClusterStatus,
    pub health: HealthStatus,
    pub databasus_status: DatabasusIntegrationStatus,
    pub delete_protection: bool,
    pub enable_backup: bool,
    pub last_error: Option<String>,
    /// Docker host / node this cluster runs on.
    #[serde(default)]
    pub node_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InitialDatabaseSpec {
    pub database_name: String,
    pub role_name: String,
    /// If omitted, a strong password is generated.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateClusterRequest {
    pub name: String,
    /// "16", "17", or "18"
    pub postgres_version: String,
    pub cpu_limit: f64,
    pub memory_mb: u32,
    pub storage_limit_gb: u32,
    #[serde(default)]
    pub expose_publicly: bool,
    pub optional_public_port: Option<u16>,
    #[serde(default)]
    pub enable_backup: bool,
    /// Target Docker node (defaults to panel local node).
    #[serde(default)]
    pub node_id: Option<Uuid>,
    /// Databases + non-superuser roles created after the cluster is healthy.
    #[serde(default)]
    pub initial_databases: Vec<InitialDatabaseSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterCreatedResponse {
    pub cluster: Cluster,
    pub operation_id: Uuid,
    /// One-time plaintext admin password — never returned again.
    pub admin_password: String,
    pub connection_info: ConnectionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConnectionInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    /// Only present when password is revealed once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

// ── Delete ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeleteMode {
    /// Remove from panel inventory only; leave Docker resources alone.
    RemoveFromPanel,
    /// Stop & remove container and network; keep volume.
    DeleteContainerKeepVolume,
    /// Destroy container, network, and volume.
    PermanentlyDelete,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeleteClusterRequest {
    pub mode: DeleteMode,
    /// Required for PermanentlyDelete: must match cluster name exactly.
    pub confirm_name: Option<String>,
    /// Required for PermanentlyDelete when wiping volume.
    #[serde(default)]
    pub confirm_volume_delete: bool,
    #[serde(default)]
    pub final_backup: bool,
}

// ── Database / Role ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DatabaseRecord {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub name: String,
    pub owner_role: String,
    pub connection_limit: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoleRecord {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub name: String,
    pub is_superuser: bool,
    pub can_login: bool,
    pub connection_limit: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDatabaseRequest {
    pub database_name: String,
    pub role_name: String,
    #[serde(default)]
    pub generate_password: bool,
    pub password: Option<String>,
    pub connection_limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDatabaseResponse {
    pub database: DatabaseRecord,
    pub role: RoleRecord,
    pub operation_id: Uuid,
    /// One-time plaintext password.
    pub password: String,
    pub connection_string: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateRoleRequest {
    pub role_name: String,
    #[serde(default)]
    pub generate_password: bool,
    pub password: Option<String>,
    pub connection_limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    #[serde(default)]
    pub generate_password: bool,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordResponse {
    pub role_name: String,
    pub password: String,
    pub warning: String,
}

// ── Browser / Query ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SchemaInfo {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub table_type: String,
    pub row_estimate: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub column: String,
    pub foreign_table_schema: String,
    pub foreign_table: String,
    pub foreign_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IndexInfo {
    pub name: String,
    pub definition: String,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConstraintInfo {
    pub name: String,
    pub constraint_type: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TableDetails {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub indexes: Vec<IndexInfo>,
    pub constraints: Vec<ConstraintInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TableRowsQuery {
    pub database: String,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    pub sort_column: Option<String>,
    pub sort_dir: Option<String>,
    pub filter_column: Option<String>,
    pub filter_value: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TableRowsResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub page: u32,
    pub page_size: u32,
    pub total_estimate: Option<i64>,
    pub truncated_fields: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SqlQueryRequest {
    pub database: String,
    pub sql: String,
    #[serde(default)]
    pub admin_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SqlQueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub duration_ms: u64,
    pub notices: Vec<String>,
}

// ── Operations / Jobs ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    CreateCluster,
    StartCluster,
    StopCluster,
    RestartCluster,
    DeleteCluster,
    /// Configure native backup for a cluster (replaces Databasus register).
    EnableBackup,
    /// Run a logical pg_dump backup now.
    RunBackup,
    /// Verify last backup with pg_restore -l.
    VerifyBackup,
    /// Restore a logical backup into a database.
    RestoreBackup,
    /// Enforce retention (delete expired backups from storage + DB).
    PruneBackups,
    CreateDatabase,
    DeleteDatabase,
    RotatePassword,
    RefreshMetrics,
    /// Probe a node Docker daemon.
    PingNode,
    /// Create / refresh a replica cluster.
    SyncReplica,
    /// Promote replica to primary.
    PromoteReplica,
    /// One-shot duplicate cluster onto a node.
    DuplicateCluster,
    /// Evaluate alert rules.
    EvaluateAlerts,
    /// @deprecated kept for queued jobs from older panel versions
    RegisterDatabasus,
}

impl JobType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateCluster => "create_cluster",
            Self::StartCluster => "start_cluster",
            Self::StopCluster => "stop_cluster",
            Self::RestartCluster => "restart_cluster",
            Self::DeleteCluster => "delete_cluster",
            Self::EnableBackup => "enable_backup",
            Self::RunBackup => "run_backup",
            Self::VerifyBackup => "verify_backup",
            Self::RestoreBackup => "restore_backup",
            Self::PruneBackups => "prune_backups",
            Self::CreateDatabase => "create_database",
            Self::DeleteDatabase => "delete_database",
            Self::RotatePassword => "rotate_password",
            Self::RefreshMetrics => "refresh_metrics",
            Self::PingNode => "ping_node",
            Self::SyncReplica => "sync_replica",
            Self::PromoteReplica => "promote_replica",
            Self::DuplicateCluster => "duplicate_cluster",
            Self::EvaluateAlerts => "evaluate_alerts",
            Self::RegisterDatabasus => "register_databasus",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create_cluster" => Some(Self::CreateCluster),
            "start_cluster" => Some(Self::StartCluster),
            "stop_cluster" => Some(Self::StopCluster),
            "restart_cluster" => Some(Self::RestartCluster),
            "delete_cluster" => Some(Self::DeleteCluster),
            "enable_backup" | "register_databasus" => Some(Self::EnableBackup),
            "run_backup" => Some(Self::RunBackup),
            "verify_backup" => Some(Self::VerifyBackup),
            "restore_backup" => Some(Self::RestoreBackup),
            "prune_backups" => Some(Self::PruneBackups),
            "create_database" => Some(Self::CreateDatabase),
            "delete_database" => Some(Self::DeleteDatabase),
            "rotate_password" => Some(Self::RotatePassword),
            "refresh_metrics" => Some(Self::RefreshMetrics),
            "ping_node" => Some(Self::PingNode),
            "sync_replica" => Some(Self::SyncReplica),
            "promote_replica" => Some(Self::PromoteReplica),
            "duplicate_cluster" => Some(Self::DuplicateCluster),
            "evaluate_alerts" => Some(Self::EvaluateAlerts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Operation {
    pub id: Uuid,
    pub job_type: JobType,
    pub status: JobStatus,
    pub cluster_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub progress: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OperationLog {
    pub id: i64,
    pub operation_id: Uuid,
    pub level: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

// ── Auth ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(default = "default_admin_role")]
    pub role: UserRole,
    #[serde(default)]
    pub display_name: String,
    #[serde(default = "default_true_user")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

fn default_admin_role() -> UserRole {
    UserRole::Admin
}
fn default_true_user() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Owner,
    Admin,
    Operator,
    Viewer,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            "operator" => Self::Operator,
            "viewer" => Self::Viewer,
            _ => Self::Admin,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Owner => 4,
            Self::Admin => 3,
            Self::Operator => 2,
            Self::Viewer => 1,
        }
    }

    pub fn can_write(self) -> bool {
        self.rank() >= Self::Operator.rank()
    }

    pub fn can_admin(self) -> bool {
        self.rank() >= Self::Admin.rank()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BootstrapRequest {
    pub username: String,
    pub password: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeResponse {
    pub user: User,
    pub csrf_token: String,
}

// ── Audit ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuditLog {
    pub id: i64,
    pub actor_id: Option<Uuid>,
    pub actor_username: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub details: serde_json::Value,
    pub ip_address: Option<String>,
    pub correlation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Backup ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupStatus {
    pub integration_status: DatabasusIntegrationStatus,
    pub last_successful_backup: Option<DateTime<Utc>>,
    pub last_backup_status: Option<String>,
    pub backup_lag_seconds: Option<i64>,
    pub wal_status: Option<String>,
    pub failed_backups: u32,
    pub message: Option<String>,
    pub manual_setup_info: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupSchedule {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub cron: String,
    pub kind: String,
    pub database_name: String,
    pub enabled: bool,
    pub retention_days: u32,
    pub keep_count: u32,
    pub compression_level: u8,
    pub dump_format: String,
    pub schema_only: bool,
    pub exclude_schemas: String,
    pub exclude_tables: String,
    pub include_schemas: String,
    pub jobs: u8,
    pub notify_on_success: bool,
    pub notify_on_failure: bool,
    pub verify_after: bool,
    pub window_start_hour: Option<u8>,
    pub window_end_hour: Option<u8>,
    pub pause_until: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertBackupScheduleRequest {
    #[serde(default = "default_cron")]
    pub cron: String,
    #[serde(default = "default_db_name")]
    pub database_name: String,
    #[serde(default = "default_true_bool")]
    pub enabled: bool,
    #[serde(default = "default_retention")]
    pub retention_days: u32,
    #[serde(default = "default_keep")]
    pub keep_count: u32,
    #[serde(default = "default_compression")]
    pub compression_level: u8,
    #[serde(default = "default_dump_format")]
    pub dump_format: String,
    #[serde(default)]
    pub schema_only: bool,
    #[serde(default)]
    pub exclude_schemas: String,
    #[serde(default)]
    pub exclude_tables: String,
    #[serde(default)]
    pub include_schemas: String,
    #[serde(default = "default_jobs")]
    pub jobs: u8,
    #[serde(default)]
    pub notify_on_success: bool,
    #[serde(default = "default_true_bool")]
    pub notify_on_failure: bool,
    #[serde(default)]
    pub verify_after: bool,
    pub window_start_hour: Option<u8>,
    pub window_end_hour: Option<u8>,
    pub description: Option<String>,
}

fn default_cron() -> String {
    "0 3 * * *".into()
}
fn default_db_name() -> String {
    "postgres".into()
}
fn default_true_bool() -> bool {
    true
}
fn default_retention() -> u32 {
    14
}
fn default_keep() -> u32 {
    30
}
fn default_compression() -> u8 {
    6
}
fn default_dump_format() -> String {
    "custom".into()
}
fn default_jobs() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RestoreBackupRequest {
    pub backup_id: Uuid,
    pub target_database: String,
    #[serde(default)]
    pub clean: bool,
    /// Must match cluster name for destructive restore.
    pub confirm_cluster_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GlobalBackupPolicy {
    pub retention_days: u32,
    pub keep_count: u32,
    pub schedule_hour: u8,
    pub cron_default: String,
    pub compression_level: u8,
    pub dump_format: String,
    pub verify_after: bool,
    pub keep_local_copy: bool,
    pub notify_webhook: String,
    pub notify_on_success: bool,
    pub notify_on_failure: bool,
    pub exclude_schemas_default: String,
    pub wal_archiving_default: bool,
    pub parallel_jobs: u8,
    pub encrypt: bool,
}

// ── Nodes ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Local,
    Remote,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "remote" => Self::Remote,
            _ => Self::Local,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Unknown,
    Online,
    Offline,
    Degraded,
    Disabled,
}

impl NodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Degraded => "degraded",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "online" => Self::Online,
            "offline" => Self::Offline,
            "degraded" => Self::Degraded,
            "disabled" => Self::Disabled,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Node {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub kind: NodeKind,
    /// Redacted for remote (shows host without credentials).
    pub docker_host_display: Option<String>,
    pub status: NodeStatus,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub max_clusters: Option<u32>,
    pub cluster_count: u32,
    pub notes: Option<String>,
    pub is_default: bool,
    pub labels: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateNodeRequest {
    pub name: String,
    /// unix:///var/run/docker.sock or tcp://host:2375 or ssh://user@host
    pub docker_host: String,
    #[serde(default)]
    pub notes: Option<String>,
    pub max_clusters: Option<u32>,
    #[serde(default)]
    pub labels: Option<serde_json::Value>,
    #[serde(default)]
    pub set_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateNodeRequest {
    pub name: Option<String>,
    pub docker_host: Option<String>,
    pub notes: Option<String>,
    pub max_clusters: Option<u32>,
    pub labels: Option<serde_json::Value>,
    pub status: Option<NodeStatus>,
    #[serde(default)]
    pub set_default: bool,
}

// ── WAF ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WafPolicyConfig {
    #[serde(default = "default_rate")]
    pub rate_limit_per_minute: u32,
    #[serde(default = "default_login_rate")]
    pub login_rate_limit_per_minute: u32,
    #[serde(default = "default_api_rate")]
    pub api_rate_limit_per_minute: u32,
    #[serde(default = "default_body")]
    pub max_body_bytes: u64,
    #[serde(default)]
    pub block_empty_user_agent: bool,
    #[serde(default)]
    pub blocked_user_agents: Vec<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub denied_ips: Vec<String>,
    #[serde(default)]
    pub blocked_paths: Vec<String>,
    #[serde(default)]
    pub challenge_suspicious: bool,
    #[serde(default)]
    pub geo_block_countries: Vec<String>,
    #[serde(default = "default_true_bool")]
    pub enable_security_headers: bool,
    #[serde(default = "default_hsts")]
    pub hsts_max_age: u64,
    #[serde(default = "default_csp")]
    pub csp_mode: String,
    #[serde(default = "default_true_bool")]
    pub fail_closed_on_deny: bool,
}

fn default_rate() -> u32 {
    120
}
fn default_login_rate() -> u32 {
    20
}
fn default_api_rate() -> u32 {
    300
}
fn default_body() -> u64 {
    10_485_760
}
fn default_hsts() -> u64 {
    31_536_000
}
fn default_csp() -> String {
    "strict".into()
}

impl Default for WafPolicyConfig {
    fn default() -> Self {
        Self {
            rate_limit_per_minute: default_rate(),
            login_rate_limit_per_minute: default_login_rate(),
            api_rate_limit_per_minute: default_api_rate(),
            max_body_bytes: default_body(),
            block_empty_user_agent: false,
            blocked_user_agents: vec![],
            allowed_ips: vec![],
            denied_ips: vec![],
            blocked_paths: vec![],
            challenge_suspicious: false,
            geo_block_countries: vec![],
            enable_security_headers: true,
            hsts_max_age: default_hsts(),
            csp_mode: default_csp(),
            fail_closed_on_deny: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WafPolicy {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub is_active: bool,
    pub version: u32,
    pub config: WafPolicyConfig,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateWafPolicyRequest {
    pub config: WafPolicyConfig,
    pub enabled: Option<bool>,
    pub notes: Option<String>,
    /// Human reason recorded in the change log.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WafChangeEntry {
    pub id: i64,
    pub policy_id: Uuid,
    pub version_from: Option<u32>,
    pub version_to: u32,
    pub actor_username: Option<String>,
    pub change_summary: String,
    pub before_json: Option<serde_json::Value>,
    pub after_json: serde_json::Value,
    pub reason: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Metrics ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterMetrics {
    pub container_running: bool,
    pub postgres_reachable: bool,
    pub cpu_percent: Option<f64>,
    pub memory_usage_mb: Option<f64>,
    pub memory_limit_mb: Option<f64>,
    pub volume_size_bytes: Option<u64>,
    pub active_connections: Option<i64>,
    pub databases: Vec<DatabaseSize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DatabaseSize {
    pub name: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardStats {
    pub cluster_count: i64,
    pub healthy_count: i64,
    pub degraded_count: i64,
    pub database_count: i64,
    pub active_operations: i64,
    pub failed_operations: i64,
    #[serde(default)]
    pub node_count: i64,
    #[serde(default)]
    pub open_alerts: i64,
    #[serde(default)]
    pub replica_count: i64,
    #[serde(default)]
    pub backup_destinations: i64,
}

// ── Backup destinations ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BackupDestination {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub storage_type: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub path_style: bool,
    pub tls_verify: bool,
    pub encrypt_backups: bool,
    pub compression_level: u8,
    pub enabled: bool,
    pub is_default: bool,
    pub access_key_set: bool,
    pub secret_key_set: bool,
    pub notes: Option<String>,
    pub last_test_at: Option<DateTime<Utc>>,
    pub last_test_ok: Option<bool>,
    pub last_test_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertBackupDestinationRequest {
    pub name: String,
    pub storage_type: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_region_dest")]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default = "default_prefix_dest")]
    pub prefix: String,
    #[serde(default = "default_true_bool")]
    pub path_style: bool,
    #[serde(default = "default_true_bool")]
    pub tls_verify: bool,
    #[serde(default = "default_true_bool")]
    pub encrypt_backups: bool,
    #[serde(default = "default_compression")]
    pub compression_level: u8,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub set_default: bool,
    #[serde(default = "default_true_bool")]
    pub enabled: bool,
    /// Optional node IDs allowed to use this destination (empty = all).
    #[serde(default)]
    pub allowed_node_ids: Vec<Uuid>,
}

fn default_region_dest() -> String {
    "auto".into()
}
fn default_prefix_dest() -> String {
    "pgpanel/".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterBackupTarget {
    pub id: Uuid,
    pub cluster_id: Uuid,
    pub destination_id: Uuid,
    pub destination_name: String,
    pub enabled: bool,
    pub priority: i32,
    pub include_databases: String,
    pub exclude_databases: String,
    pub cron: String,
    pub retention_days: u32,
    pub keep_count: u32,
    pub schema_only: bool,
    pub verify_after: bool,
    pub compression_level: Option<u8>,
    pub dump_format: String,
    pub exclude_schemas: String,
    pub exclude_tables: String,
    pub parallel_jobs: u8,
    pub notify_on_success: bool,
    pub notify_on_failure: bool,
    pub window_start_hour: Option<u8>,
    pub window_end_hour: Option<u8>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpsertClusterBackupTargetRequest {
    pub destination_id: Uuid,
    #[serde(default = "default_true_bool")]
    pub enabled: bool,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default = "default_include_dbs")]
    pub include_databases: String,
    #[serde(default)]
    pub exclude_databases: String,
    #[serde(default = "default_cron")]
    pub cron: String,
    #[serde(default = "default_retention")]
    pub retention_days: u32,
    #[serde(default = "default_keep")]
    pub keep_count: u32,
    #[serde(default)]
    pub schema_only: bool,
    #[serde(default)]
    pub verify_after: bool,
    pub compression_level: Option<u8>,
    #[serde(default = "default_dump_format")]
    pub dump_format: String,
    #[serde(default)]
    pub exclude_schemas: String,
    #[serde(default)]
    pub exclude_tables: String,
    #[serde(default = "default_jobs")]
    pub parallel_jobs: u8,
    #[serde(default)]
    pub notify_on_success: bool,
    #[serde(default = "default_true_bool")]
    pub notify_on_failure: bool,
    pub window_start_hour: Option<u8>,
    pub window_end_hour: Option<u8>,
}

fn default_priority() -> i32 {
    100
}
fn default_include_dbs() -> String {
    "*".into()
}

// ── Replicas ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterReplica {
    pub id: Uuid,
    pub primary_cluster_id: Uuid,
    pub replica_cluster_id: Option<Uuid>,
    pub name: String,
    pub mode: String,
    pub target_node_id: Uuid,
    pub sync_cron: String,
    pub status: String,
    pub lag_seconds: Option<i64>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub auto_failover: bool,
    pub promote_protection: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateReplicaRequest {
    pub name: String,
    pub target_node_id: Uuid,
    #[serde(default = "default_replica_mode")]
    pub mode: String,
    #[serde(default = "default_sync_cron")]
    pub sync_cron: String,
    #[serde(default)]
    pub auto_failover: bool,
    /// Create the replica cluster immediately.
    #[serde(default = "default_true_bool")]
    pub provision_now: bool,
}

fn default_replica_mode() -> String {
    "scheduled_sync".into()
}
fn default_sync_cron() -> String {
    "*/30 * * * *".into()
}

// ── Users admin ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub role: UserRole,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub role: Option<UserRole>,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
    pub password: Option<String>,
}

// ── Alerts / monitoring ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AlertRule {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub severity: String,
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
    pub duration_seconds: u32,
    pub scope: String,
    pub scope_id: Option<String>,
    pub notify_channels: String,
    pub cooldown_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Alert {
    pub id: Uuid,
    pub rule_id: Option<Uuid>,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub status: String,
    pub fired_at: DateTime<Utc>,
    pub acked_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MonitoringOverview {
    pub cluster_count: i64,
    pub healthy_count: i64,
    pub open_alerts: i64,
    pub avg_cpu: f64,
    pub avg_memory_mb: f64,
    pub backups_last_24h: i64,
    pub failed_backups_24h: i64,
    pub replica_healthy: i64,
    pub replica_total: i64,
    pub series: Vec<MonitoringPoint>,
    pub top_clusters: Vec<ClusterLoadRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MonitoringPoint {
    pub at: String,
    pub cpu: f64,
    pub memory_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClusterLoadRow {
    pub cluster_id: Uuid,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiTokenCreated {
    pub id: Uuid,
    pub name: String,
    pub token: String,
    pub token_prefix: String,
    pub role: String,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiTokenInfo {
    pub id: Uuid,
    pub name: String,
    pub token_prefix: String,
    pub role: String,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApiTokenRequest {
    pub name: String,
    #[serde(default = "default_token_role")]
    pub role: String,
    #[serde(default = "default_scopes")]
    pub scopes: String,
    pub expires_days: Option<u32>,
}

fn default_token_role() -> String {
    "operator".into()
}
fn default_scopes() -> String {
    "read,write".into()
}

// ── API envelope ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadyResponse {
    pub ready: bool,
    pub database: bool,
    pub docker: bool,
}
