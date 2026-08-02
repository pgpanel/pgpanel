use utoipa::OpenApi;

use pgpanel_core::models::*;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "PgPanel API",
        version = "0.1.0",
        description = "Self-hosted PostgreSQL management panel"
    ),
    paths(),
    components(schemas(
        Cluster,
        CreateClusterRequest,
        ClusterCreatedResponse,
        ConnectionInfo,
        DeleteClusterRequest,
        DeleteMode,
        DatabaseRecord,
        RoleRecord,
        CreateDatabaseRequest,
        CreateDatabaseResponse,
        CreateRoleRequest,
        ChangePasswordRequest,
        ChangePasswordResponse,
        SchemaInfo,
        TableInfo,
        TableDetails,
        TableRowsResponse,
        SqlQueryRequest,
        SqlQueryResponse,
        Operation,
        OperationLog,
        User,
        BootstrapRequest,
        LoginRequest,
        MeResponse,
        AuditLog,
        BackupStatus,
        ClusterMetrics,
        DashboardStats,
        ApiErrorBody,
        HealthResponse,
        ReadyResponse,
        ClusterStatus,
        HealthStatus,
        JobType,
        JobStatus,
    )),
    tags(
        (name = "auth", description = "Authentication"),
        (name = "clusters", description = "PostgreSQL clusters"),
        (name = "databases", description = "Databases and roles"),
        (name = "browser", description = "Read-only database browser"),
        (name = "operations", description = "Background jobs"),
        (name = "system", description = "Health and readiness"),
    )
)]
pub struct ApiDoc;
