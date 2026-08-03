//! Askama view models and template helpers.

use askama::Template;
use pgpanel_core::permissions::Role;
use pgpanel_protocol::{ClusterDetail, ClusterSummary};
use serde::Serialize;

/// Flash message for UI.
#[derive(Debug, Clone, Serialize)]
pub struct FlashMessage {
    pub level: String,
    pub message: String,
}

/// Breadcrumb item.
#[derive(Debug, Clone, Serialize)]
pub struct Breadcrumb {
    pub label: String,
    pub href: Option<String>,
}

/// Pagination info.
#[derive(Debug, Clone, Serialize)]
pub struct Pagination {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
}

impl Pagination {
    pub fn new(page: i64, per_page: i64, total: i64) -> Self {
        let total_pages = (total + per_page - 1) / per_page;
        Self {
            page,
            per_page,
            total,
            total_pages: total_pages.max(1),
        }
    }

    pub fn has_prev(&self) -> bool {
        self.page > 1
    }

    pub fn has_next(&self) -> bool {
        self.page < self.total_pages
    }

    pub fn prev_page(&self) -> i64 {
        (self.page - 1).max(1)
    }

    pub fn next_page(&self) -> i64 {
        (self.page + 1).min(self.total_pages)
    }
}

/// Base layout context.
#[derive(Debug, Clone)]
pub struct LayoutCtx {
    pub title: String,
    pub username: Option<String>,
    pub role: Option<String>,
    pub csrf_token: String,
    pub flash: Option<FlashMessage>,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub active_nav: String,
    pub asset_v: String,
}

/// Auth layout context.
#[derive(Debug, Clone)]
pub struct AuthLayoutCtx {
    pub title: String,
    pub flash: Option<FlashMessage>,
    pub asset_v: String,
}

#[derive(Template)]
#[template(path = "pages/auth/login.html")]
pub struct LoginPage<'a> {
    pub ctx: &'a AuthLayoutCtx,
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/auth/setup.html")]
pub struct SetupPage<'a> {
    pub ctx: &'a AuthLayoutCtx,
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/auth/totp.html")]
pub struct TotpPage<'a> {
    pub ctx: &'a AuthLayoutCtx,
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/auth/security.html")]
pub struct SecurityPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub enabled: bool,
    pub otpauth_uri: Option<&'a str>,
    pub manual_secret: Option<&'a str>,
    pub backup_codes: &'a [String],
    pub error: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/auth/reauth.html")]
pub struct ReauthPage<'a> {
    pub ctx: &'a AuthLayoutCtx,
    pub csrf_token: &'a str,
    pub return_to: &'a str,
    pub error: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/dashboard/index.html")]
pub struct DashboardPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub cluster_count: usize,
    pub online_count: usize,
    pub samples: Vec<crate::services::monitoring::MonitoringSample>,
    pub recent_audit: Vec<crate::services::audit_service::AuditRow>,
}

#[derive(Template)]
#[template(path = "pages/clusters/list.html")]
pub struct ClustersListPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub clusters: Vec<ClusterSummary>,
}

#[derive(Template)]
#[template(path = "pages/clusters/detail.html")]
pub struct ClusterDetailPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub cluster: ClusterDetail,
    pub has_credentials: bool,
}

#[derive(Template)]
#[template(path = "pages/clusters/create.html")]
pub struct ClusterCreatePage<'a> {
    pub ctx: &'a LayoutCtx,
    pub allowed_versions: Vec<String>,
}

#[derive(Template)]
#[template(path = "pages/clusters/delete_confirm.html")]
pub struct ClusterDeletePage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub name: String,
    pub confirm_phrase: String,
}

#[derive(Template)]
#[template(path = "pages/clusters/config.html")]
pub struct ClusterConfigPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub name: String,
    pub settings: Vec<pgpanel_protocol::ConfigSetting>,
    pub config_path: String,
}

#[derive(Template)]
#[template(path = "pages/clusters/logs.html")]
pub struct ClusterLogsPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub name: String,
    pub lines: Vec<String>,
    pub log_path: String,
    pub truncated: bool,
}

#[derive(Template)]
#[template(path = "pages/databases/list.html")]
pub struct DatabasesListPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub port: u16,
    pub databases: Vec<crate::services::postgres_admin::DatabaseInfo>,
}

#[derive(Template)]
#[template(path = "pages/databases/create.html")]
pub struct DatabaseCreatePage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub port: u16,
}

#[derive(Template)]
#[template(path = "pages/databases/detail.html")]
pub struct DatabaseDetailPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub database: crate::services::postgres_admin::DatabaseInfo,
}

#[derive(Template)]
#[template(path = "pages/roles/list.html")]
pub struct RolesListPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub port: u16,
    pub roles: Vec<crate::services::postgres_admin::RoleInfo>,
}

#[derive(Template)]
#[template(path = "pages/roles/create.html")]
pub struct RoleCreatePage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub port: u16,
    pub allow_superuser: bool,
}

#[derive(Template)]
#[template(path = "pages/roles/detail.html")]
pub struct RoleDetailPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub role: crate::services::postgres_admin::RoleInfo,
}

#[derive(Template)]
#[template(path = "pages/sql/editor.html")]
pub struct SqlEditorPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub version: String,
    pub cluster: String,
    pub port: u16,
    pub database: String,
    pub result: Option<crate::services::postgres_admin::QueryResult>,
    pub query: String,
    pub read_only: bool,
}

#[derive(Template)]
#[template(path = "pages/monitoring/index.html")]
pub struct MonitoringPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub samples: Vec<crate::services::monitoring::MonitoringSample>,
}

#[derive(Template)]
#[template(path = "pages/logs/index.html")]
pub struct LogsPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub clusters: Vec<ClusterSummary>,
}

#[derive(Template)]
#[template(path = "pages/backups/index.html")]
pub struct BackupsPage<'a> {
    pub ctx: &'a LayoutCtx,
    /// Integration toggle from config.
    pub enabled: bool,
    /// Enabled and `base_url` is set.
    pub configured: bool,
    /// `healthy`, `degraded`, or `unavailable`.
    pub health_status: &'a str,
    pub health_message: Option<String>,
    /// Job listing requires a future adapter (`database_id` mapping).
    pub job_listing_supported: bool,
    /// Trigger requires a future adapter; not exposed via public health API.
    pub trigger_supported: bool,
    /// Restore verification requires a future adapter.
    pub restore_verification_supported: bool,
}

#[derive(Template)]
#[template(path = "pages/audit/index.html")]
pub struct AuditPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub entries: Vec<crate::services::audit_service::AuditRow>,
    pub pagination: Pagination,
    pub action_filter: String,
}

#[derive(Template)]
#[template(path = "pages/users/list.html")]
pub struct UsersListPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub users: Vec<UserRow>,
}

#[derive(Template)]
#[template(path = "pages/users/create.html")]
pub struct UserCreatePage<'a> {
    pub ctx: &'a LayoutCtx,
    pub roles: Vec<&'static str>,
}

#[derive(Template)]
#[template(path = "pages/settings/index.html")]
pub struct SettingsPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub settings: Vec<SettingRow>,
}

#[derive(Template)]
#[template(path = "pages/updates/index.html")]
pub struct UpdatesPage<'a> {
    pub ctx: &'a LayoutCtx,
    pub status: &'a crate::services::updates_service::UpdatePageStatus,
    pub notice: Option<&'a str>,
    pub csrf_token: &'a str,
}

#[derive(Template)]
#[template(path = "pages/updates/status_fragment.html")]
pub struct UpdatesStatusFragment<'a> {
    pub status: &'a crate::services::updates_service::UpdatePageStatus,
    pub csrf_token: &'a str,
}

#[derive(Template)]
#[template(path = "components/flash.html")]
pub struct FlashPartial<'a> {
    pub flash: &'a FlashMessage,
}

#[derive(Template)]
#[template(path = "components/status_badge.html")]
pub struct StatusBadge<'a> {
    pub status: &'a str,
}

#[derive(Template)]
#[template(path = "components/empty_state.html")]
pub struct EmptyState<'a> {
    pub title: &'a str,
    pub message: &'a str,
}

#[derive(Template)]
#[template(path = "partials/cluster_row.html")]
pub struct ClusterRowPartial<'a> {
    pub cluster: &'a ClusterSummary,
}

/// User row for admin UI.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub totp_enabled: i64,
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub disabled: i64,
}

/// Setting row for display (secrets redacted).
#[derive(Debug, Clone, Serialize)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

/// Build layout context from auth user.
pub fn layout_ctx(
    title: impl Into<String>,
    user: &crate::middleware::auth::AuthUser,
    active_nav: impl Into<String>,
) -> LayoutCtx {
    LayoutCtx {
        title: title.into(),
        username: Some(user.username().to_string()),
        role: Some(user.role().as_str().to_string()),
        csrf_token: user.session.csrf_token.clone(),
        flash: None,
        breadcrumbs: vec![],
        active_nav: active_nav.into(),
        asset_v: asset_version().to_string(),
    }
}

pub fn auth_layout(title: impl Into<String>) -> AuthLayoutCtx {
    AuthLayoutCtx {
        title: title.into(),
        flash: None,
        asset_v: asset_version().to_string(),
    }
}

fn asset_version() -> &'static str {
    pgpanel_core::VERSION
}

pub fn role_label(role: Role) -> &'static str {
    role.as_str()
}

pub fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

pub fn status_class(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "online" | "running" | "success" | "active" => {
            "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/30 dark:text-emerald-300"
        }
        "down" | "stopped" | "failure" | "error" => {
            "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-300"
        }
        "restarting" | "pending" | "warning" => {
            "bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300"
        }
        _ => "bg-slate-100 text-slate-800 dark:bg-slate-800 dark:text-slate-300",
    }
}
