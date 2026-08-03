//! Cluster management routes.

use crate::error::{AppError, AppResult};
use crate::helper_client::HelperClientError;
use crate::middleware::auth::AuthUser;
use crate::middleware::request_id::{ClientIp, RequestId};
use crate::state::AppState;
use crate::templates_data::{
    layout_ctx, ClusterConfigPage, ClusterCreatePage, ClusterDeletePage, ClusterDetailPage,
    ClusterLogsPage, ClustersListPage,
};
use askama::Template;
use axum::{
    extract::{Extension, Path, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Form, Router,
};
use pgpanel_core::audit::AuditEvent;
use pgpanel_core::permissions::Permission;
use pgpanel_core::validation::{validate_cluster_name, validate_pg_version};
use pgpanel_protocol::{ClusterCreateParams, ConfigSetting, HelperOk, HelperOp, HelperResult};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/create", get(create_form).post(create_submit))
        .route("/:version/:name", get(detail))
        .route("/:version/:name/start", post(start))
        .route("/:version/:name/stop", post(stop))
        .route("/:version/:name/restart", post(restart))
        .route("/:version/:name/reload", post(reload))
        .route("/:version/:name/rename", post(rename))
        .route(
            "/:version/:name/delete",
            get(delete_form).post(delete_submit),
        )
        .route(
            "/:version/:name/config",
            get(config_view).post(config_update),
        )
        .route("/:version/:name/logs", get(logs_view))
        .route("/:version/:name/credentials", post(save_credentials))
}

async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::ClustersRead)?;
    let clusters = helper_clusters(&state, &user).await?;
    let ctx = layout_ctx("Clusters", &user, "clusters");
    let page = ClustersListPage {
        ctx: &ctx,
        clusters,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
) -> AppResult<Html<String>> {
    user.require(Permission::ClustersRead)?;
    let cluster = helper_inspect(&state, &user, &version, &name).await?;
    let has_credentials = state
        .postgres
        .get_credentials(&version, &name)
        .await?
        .is_some();
    let ctx = layout_ctx(format!("{version}/{name}"), &user, "clusters");
    let page = ClusterDetailPage {
        ctx: &ctx,
        cluster,
        has_credentials,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

async fn create_form(State(state): State<AppState>, user: AuthUser) -> AppResult<Html<String>> {
    user.require(Permission::ClustersCreate)?;
    render_create_page(&state, &user, None, None).await
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct CreateForm {
    csrf_token: String,
    version: String,
    name: String,
    port: u16,
    encoding: String,
    locale: String,
    data_checksums: Option<String>,
    start: Option<String>,
    listen_addresses: String,
}

async fn create_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Extension(ip): Extension<ClientIp>,
    Extension(req_id): Extension<RequestId>,
    Form(form): Form<CreateForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersCreate)?;
    validate_pg_version(&form.version)?;
    validate_cluster_name(&form.name)?;
    let source_ip = ip.0.clone();

    let params = ClusterCreateParams {
        version: form.version.clone(),
        name: form.name.clone(),
        display_name: None,
        port: form.port,
        encoding: form.encoding,
        locale: form.locale,
        data_checksums: form.data_checksums.is_some(),
        start: form.start.is_some(),
        listen_addresses: form.listen_addresses,
        data_directory: None,
    };

    let resp = state
        .helper
        .call(
            HelperOp::ClusterCreate { params },
            Some(user.user_id()),
            Some(user.username().to_string()),
            Some(source_ip.clone()),
            Some(std::time::Duration::from_secs(
                state.config.timeouts.cluster_create_secs,
            )),
        )
        .await
        .map_err(map_helper_err)?;

    match resp.result {
        HelperResult::Ok { .. } => {
            state
                .audit
                .record(AuditEvent::success(
                    "cluster.create",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(source_ip),
                    req_id.0,
                    Some(format!("{}/{}", form.version, form.name)),
                    None,
                ))
                .await
                .ok();
            Ok(Redirect::to(&format!("/clusters/{}/{}", form.version, form.name)).into_response())
        }
        HelperResult::Err { error } => {
            let err = AppError::from_helper_error(error);
            render_create_page(&state, &user, Some(err.user_message()), err.details())
                .await
                .map(IntoResponse::into_response)
        }
    }
}

async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Extension(ip): Extension<ClientIp>,
) -> AppResult<impl IntoResponse> {
    control_action(
        &state,
        &user,
        &version,
        &name,
        "start",
        HelperOp::ClusterStart {
            version: version.clone(),
            name: name.clone(),
        },
        Some(ip.0),
    )
    .await
}

async fn stop(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Extension(ip): Extension<ClientIp>,
) -> AppResult<impl IntoResponse> {
    control_action(
        &state,
        &user,
        &version,
        &name,
        "stop",
        HelperOp::ClusterStop {
            version: version.clone(),
            name: name.clone(),
            force: false,
        },
        Some(ip.0),
    )
    .await
}

async fn restart(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Extension(ip): Extension<ClientIp>,
) -> AppResult<impl IntoResponse> {
    control_action(
        &state,
        &user,
        &version,
        &name,
        "restart",
        HelperOp::ClusterRestart {
            version: version.clone(),
            name: name.clone(),
        },
        Some(ip.0),
    )
    .await
}

async fn reload(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Extension(ip): Extension<ClientIp>,
) -> AppResult<impl IntoResponse> {
    control_action(
        &state,
        &user,
        &version,
        &name,
        "reload",
        HelperOp::ClusterReload {
            version: version.clone(),
            name: name.clone(),
        },
        Some(ip.0),
    )
    .await
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct RenameForm {
    csrf_token: String,
    new_name: String,
}

async fn rename(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Form(form): Form<RenameForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersConfigure)?;
    validate_cluster_name(&form.new_name)?;
    let resp = state
        .helper
        .call(
            HelperOp::ClusterRename {
                version: version.clone(),
                old_name: name.clone(),
                new_name: form.new_name.clone(),
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            None,
        )
        .await
        .map_err(map_helper_err)?;
    match resp.result {
        HelperResult::Ok { .. } => {
            Ok(Redirect::to(&format!("/clusters/{}/{}", version, form.new_name)).into_response())
        }
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

async fn delete_form(
    State(_state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
) -> AppResult<Html<String>> {
    user.require(Permission::ClustersDelete)?;
    let confirm_phrase = format!("DELETE {name}");
    let ctx = layout_ctx("Delete Cluster", &user, "clusters");
    let page = ClusterDeletePage {
        ctx: &ctx,
        version,
        name,
        confirm_phrase,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct DeleteForm {
    csrf_token: String,
    confirmation: String,
    stop_first: Option<String>,
}

async fn delete_submit(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Extension(ip): Extension<ClientIp>,
    Extension(req_id): Extension<RequestId>,
    Form(form): Form<DeleteForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersDelete)?;
    require_reauth(&user, &state)?;

    let ip_str = ip.0.clone();
    let resp = state
        .helper
        .call(
            HelperOp::ClusterDelete {
                version: version.clone(),
                name: name.clone(),
                confirmation: form.confirmation,
                stop_first: form.stop_first.is_some(),
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            Some(ip_str.clone()),
            Some(std::time::Duration::from_secs(
                state.config.timeouts.cluster_delete_secs,
            )),
        )
        .await
        .map_err(map_helper_err)?;

    match resp.result {
        HelperResult::Ok { .. } => {
            state
                .audit
                .record(AuditEvent::success(
                    "cluster.delete",
                    Some(user.user_id()),
                    Some(user.username().to_string()),
                    Some(ip_str),
                    req_id.0,
                    Some(format!("{version}/{name}")),
                    None,
                ))
                .await
                .ok();
            Ok(Redirect::to("/clusters").into_response())
        }
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

async fn config_view(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
) -> AppResult<Html<String>> {
    user.require(Permission::ClustersConfigure)?;
    let resp = state
        .helper
        .call(
            HelperOp::ClusterReadConfig {
                version: version.clone(),
                name: name.clone(),
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            None,
        )
        .await
        .map_err(map_helper_err)?;

    let (settings, config_path) = match resp.result {
        HelperResult::Ok { data } => match *data {
            HelperOk::ClusterConfig {
                settings,
                config_path,
                ..
            } => (settings, config_path),
            _ => return Err(AppError::Internal("unexpected helper response".into())),
        },
        HelperResult::Err { error } => return Err(AppError::BadRequest(error.message)),
    };

    let ctx = layout_ctx("Cluster Config", &user, "clusters");
    let page = ClusterConfigPage {
        ctx: &ctx,
        version,
        name,
        settings,
        config_path,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct ConfigUpdateForm {
    csrf_token: String,
    key: String,
    value: String,
    allow_restart: Option<String>,
}

async fn config_update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Form(form): Form<ConfigUpdateForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersConfigure)?;
    let resp = state
        .helper
        .call(
            HelperOp::ClusterUpdateSafeConfig {
                version: version.clone(),
                name: name.clone(),
                settings: vec![ConfigSetting {
                    key: form.key,
                    value: form.value,
                }],
                allow_restart: form.allow_restart.is_some(),
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            Some(std::time::Duration::from_secs(
                state.config.timeouts.config_update_secs,
            )),
        )
        .await
        .map_err(map_helper_err)?;

    match resp.result {
        HelperResult::Ok { .. } => {
            Ok(Redirect::to(&format!("/clusters/{version}/{name}/config")).into_response())
        }
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

async fn logs_view(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
) -> AppResult<Html<String>> {
    user.require(Permission::LogsRead)?;
    let resp = state
        .helper
        .call(
            HelperOp::ClusterReadLogs {
                version: version.clone(),
                name: name.clone(),
                max_lines: 500,
                level_filter: None,
                search: None,
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            Some(std::time::Duration::from_secs(
                state.config.timeouts.log_read_secs,
            )),
        )
        .await
        .map_err(map_helper_err)?;

    let (lines, log_path, truncated) = match resp.result {
        HelperResult::Ok { data } => match *data {
            HelperOk::ClusterLogs {
                lines,
                log_path,
                truncated,
                ..
            } => (lines, log_path, truncated),
            _ => return Err(AppError::Internal("unexpected helper response".into())),
        },
        HelperResult::Err { error } => return Err(AppError::BadRequest(error.message)),
    };

    let ctx = layout_ctx("Cluster Logs", &user, "logs");
    let page = ClusterLogsPage {
        ctx: &ctx,
        version,
        name,
        lines,
        log_path,
        truncated,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
#[allow(dead_code)] // csrf_token validated by CSRF middleware
struct CredentialsForm {
    csrf_token: String,
    host: String,
    port: u16,
    username: String,
    database: String,
    connection_mode: String,
    password: Option<String>,
    socket_dir: Option<String>,
}

async fn save_credentials(
    State(state): State<AppState>,
    user: AuthUser,
    Path((version, name)): Path<(String, String)>,
    Form(form): Form<CredentialsForm>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersConfigure)?;
    state
        .postgres
        .save_credentials(
            &version,
            &name,
            &form.host,
            form.port,
            &form.username,
            &form.database,
            &form.connection_mode,
            form.password.as_deref(),
            form.socket_dir.as_deref(),
        )
        .await
        .map_err(AppError::from_core)?;
    Ok(Redirect::to(&format!("/clusters/{version}/{name}")).into_response())
}

async fn helper_clusters(
    state: &AppState,
    user: &AuthUser,
) -> AppResult<Vec<pgpanel_protocol::ClusterSummary>> {
    let resp = state
        .helper
        .call(
            HelperOp::ClusterList,
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            None,
        )
        .await
        .map_err(map_helper_err)?;
    match resp.result {
        HelperResult::Ok { data } => match *data {
            HelperOk::ClusterList { clusters } => Ok(clusters),
            _ => Err(AppError::Internal("unexpected helper response".into())),
        },
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

async fn helper_inspect(
    state: &AppState,
    user: &AuthUser,
    version: &str,
    name: &str,
) -> AppResult<pgpanel_protocol::ClusterDetail> {
    let resp = state
        .helper
        .call(
            HelperOp::ClusterInspect {
                version: version.to_string(),
                name: name.to_string(),
            },
            Some(user.user_id()),
            Some(user.username().to_string()),
            None,
            None,
        )
        .await
        .map_err(map_helper_err)?;
    match resp.result {
        HelperResult::Ok { data } => match *data {
            HelperOk::ClusterInspect { cluster } => Ok(cluster),
            _ => Err(AppError::Internal("unexpected helper response".into())),
        },
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

async fn control_action(
    state: &AppState,
    user: &AuthUser,
    version: &str,
    name: &str,
    _action: &str,
    op: HelperOp,
    ip: Option<String>,
) -> AppResult<impl IntoResponse> {
    user.require(Permission::ClustersControl)?;
    let resp = state
        .helper
        .call(
            op,
            Some(user.user_id()),
            Some(user.username().to_string()),
            ip,
            Some(std::time::Duration::from_secs(
                state.config.timeouts.cluster_control_secs,
            )),
        )
        .await
        .map_err(map_helper_err)?;
    match resp.result {
        HelperResult::Ok { .. } => {
            Ok(Redirect::to(&format!("/clusters/{version}/{name}")).into_response())
        }
        HelperResult::Err { error } => Err(AppError::BadRequest(error.message)),
    }
}

fn require_reauth(user: &AuthUser, state: &AppState) -> AppResult<()> {
    if !user.is_recently_authed(state.config.session.reauth_window_secs) {
        return Err(AppError::Unauthorized(
            "recent authentication required".into(),
        ));
    }
    Ok(())
}

fn map_helper_err(e: HelperClientError) -> AppError {
    match e {
        HelperClientError::Helper(msg) => AppError::BadRequest(msg),
        HelperClientError::Connect(msg) => AppError::Internal(format!("helper unavailable: {msg}")),
        HelperClientError::Timeout => AppError::Internal("helper request timed out".into()),
        HelperClientError::Closed => AppError::Internal("helper connection closed".into()),
        HelperClientError::Protocol(e) => AppError::Helper(e),
    }
}

async fn render_create_page(
    state: &AppState,
    user: &AuthUser,
    error: Option<String>,
    error_details: Option<&str>,
) -> AppResult<Html<String>> {
    let ctx = layout_ctx("Create Cluster", user, "clusters");
    let page = ClusterCreatePage {
        ctx: &ctx,
        allowed_versions: state.config.postgres.allowed_versions.clone(),
        error: error.as_deref(),
        error_details,
    };
    Ok(Html(
        page.render()
            .map_err(|e| AppError::Internal(e.to_string()))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_protocol::{HelperErrorBody, HelperErrorCode};

    #[test]
    fn helper_create_error_preserves_details_for_form_render() {
        let err = AppError::from_helper_error(HelperErrorBody {
            code: HelperErrorCode::CommandFailed,
            message: "initdb: permission denied".into(),
            details: Some("initdb: permission denied".into()),
        });
        assert_eq!(err.user_message(), "initdb: permission denied");
        assert_eq!(err.details(), Some("initdb: permission denied"));
    }
}
