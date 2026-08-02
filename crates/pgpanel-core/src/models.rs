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
    CreateDatabase,
    DeleteDatabase,
    RotatePassword,
    RefreshMetrics,
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
            Self::CreateDatabase => "create_database",
            Self::DeleteDatabase => "delete_database",
            Self::RotatePassword => "rotate_password",
            Self::RefreshMetrics => "refresh_metrics",
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
            "create_database" => Some(Self::CreateDatabase),
            "delete_database" => Some(Self::DeleteDatabase),
            "rotate_password" => Some(Self::RotatePassword),
            "refresh_metrics" => Some(Self::RefreshMetrics),
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
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
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
