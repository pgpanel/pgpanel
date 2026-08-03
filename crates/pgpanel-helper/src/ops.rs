//! Dispatch typed helper operations.

use pgpanel_core::audit::AuditEvent;
use pgpanel_core::config::Config;
use pgpanel_protocol::{
    Envelope, HelperErrorBody, HelperErrorCode, HelperOk, HelperOp, HelperResponse, HelperResult,
    PROTOCOL_VERSION,
};
use std::time::Instant;
use tracing::warn;
use uuid::Uuid;

use crate::audit::AuditWriter;
use crate::cluster::{
    cluster_create, cluster_delete, cluster_inspect, cluster_list, cluster_read_config,
    cluster_read_logs, cluster_reload, cluster_rename, cluster_restart, cluster_start,
    cluster_stop, helper_err, ClusterContext,
};
use crate::config_edit;

/// Shared helper state for request handling.
pub struct HelperState {
    /// Loaded configuration.
    pub config: Config,
    /// UID allowed to connect.
    pub allowed_uid: u32,
    /// Development mode relaxes peer checks.
    pub dev_mode: bool,
    /// Process start instant for uptime reporting.
    pub started_at: Instant,
    /// Audit writer.
    pub audit: AuditWriter,
}

impl HelperState {
    /// Create helper state.
    pub fn new(config: Config, allowed_uid: u32, dev_mode: bool, audit: AuditWriter) -> Self {
        Self {
            config,
            allowed_uid,
            dev_mode,
            started_at: Instant::now(),
            audit,
        }
    }
}

/// Handle a single protocol envelope and return a response.
pub async fn dispatch(state: &HelperState, envelope: Envelope) -> HelperResponse {
    let request_id = envelope.request_id;
    let action = op_action_name(&envelope.op);
    let target = op_target(&envelope.op);
    let actor_user_id = envelope.actor_user_id;
    let actor_username = envelope.actor_username.clone();
    let source_ip = envelope.source_ip.clone();

    let result = match execute_op(state, envelope).await {
        Ok(data) => {
            let event = AuditEvent::success(
                action,
                actor_user_id,
                actor_username,
                source_ip,
                request_id.to_string(),
                target.clone(),
                None,
            );
            state.audit.record(event).await;
            HelperResult::Ok {
                data: Box::new(data),
            }
        }
        Err(error) => {
            let event = AuditEvent::failure(
                action,
                actor_user_id,
                actor_username,
                source_ip,
                request_id.to_string(),
                target,
                error.message.clone(),
            );
            state.audit.record(event).await;
            HelperResult::Err { error }
        }
    };

    HelperResponse { request_id, result }
}

async fn execute_op(state: &HelperState, envelope: Envelope) -> Result<HelperOk, HelperErrorBody> {
    if envelope.version != PROTOCOL_VERSION {
        return Err(helper_err(
            HelperErrorCode::Protocol,
            format!(
                "unsupported protocol version {} (expected {PROTOCOL_VERSION})",
                envelope.version
            ),
            None,
        ));
    }

    let ctx = ClusterContext::new(&state.config);

    match envelope.op {
        HelperOp::Ping => Ok(HelperOk::Empty),
        HelperOp::ServiceStatus => Ok(HelperOk::ServiceStatus {
            version: pgpanel_core::VERSION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            uptime_secs: state.started_at.elapsed().as_secs(),
            allowed_uid: state.allowed_uid,
        }),
        HelperOp::ClusterList => {
            let clusters = cluster_list(&ctx).await?;
            Ok(HelperOk::ClusterList { clusters })
        }
        HelperOp::ClusterInspect { version, name } => {
            let cluster = cluster_inspect(&ctx, &version, &name).await?;
            Ok(HelperOk::ClusterInspect { cluster })
        }
        HelperOp::ClusterCreate { params } => {
            let cluster = cluster_create(&ctx, &params).await?;
            Ok(HelperOk::ClusterCreated { cluster })
        }
        HelperOp::ClusterStart { version, name } => {
            let status = cluster_start(&ctx, &version, &name).await?;
            Ok(HelperOk::ClusterAction {
                version,
                name,
                action: "start".into(),
                status: Some(status),
            })
        }
        HelperOp::ClusterStop {
            version,
            name,
            force,
        } => {
            let status = cluster_stop(&ctx, &version, &name, force).await?;
            Ok(HelperOk::ClusterAction {
                version,
                name,
                action: if force {
                    "stop_immediate".into()
                } else {
                    "stop".into()
                },
                status: Some(status),
            })
        }
        HelperOp::ClusterRestart { version, name } => {
            let status = cluster_restart(&ctx, &version, &name).await?;
            Ok(HelperOk::ClusterAction {
                version,
                name,
                action: "restart".into(),
                status: Some(status),
            })
        }
        HelperOp::ClusterReload { version, name } => {
            let status = cluster_reload(&ctx, &version, &name).await?;
            Ok(HelperOk::ClusterAction {
                version,
                name,
                action: "reload".into(),
                status: Some(status),
            })
        }
        HelperOp::ClusterRename {
            version,
            old_name,
            new_name,
        } => {
            cluster_rename(&ctx, &version, &old_name, &new_name).await?;
            Ok(HelperOk::ClusterRenamed {
                version,
                old_name,
                new_name,
            })
        }
        HelperOp::ClusterDelete {
            version,
            name,
            confirmation,
            stop_first,
        } => {
            let (data_directory, config_directory) =
                cluster_delete(&ctx, &version, &name, &confirmation, stop_first).await?;
            Ok(HelperOk::ClusterDeleted {
                version,
                name,
                data_directory,
                config_directory,
            })
        }
        HelperOp::ClusterReadConfig { version, name } => {
            let (config_path, settings) = cluster_read_config(&ctx, &version, &name).await?;
            Ok(HelperOk::ClusterConfig {
                version,
                name,
                settings,
                config_path: config_path.display().to_string(),
            })
        }
        HelperOp::ClusterUpdateSafeConfig {
            version,
            name,
            settings,
            allow_restart,
        } => {
            let (backup_path, restarted, applied) =
                config_edit::update_safe_config(&ctx, &version, &name, &settings, allow_restart)
                    .await?;
            Ok(HelperOk::ClusterConfigUpdated {
                version,
                name,
                restarted,
                backup_path: backup_path.display().to_string(),
                settings: applied,
            })
        }
        HelperOp::ClusterReadLogs {
            version,
            name,
            max_lines,
            level_filter,
            search,
        } => {
            let (log_path, lines, truncated) = cluster_read_logs(
                &ctx,
                &version,
                &name,
                max_lines,
                level_filter.as_deref(),
                search.as_deref(),
            )
            .await?;
            Ok(HelperOk::ClusterLogs {
                version,
                name,
                log_path,
                lines,
                truncated,
            })
        }
    }
}

fn op_action_name(op: &HelperOp) -> &'static str {
    match op {
        HelperOp::Ping => "helper.ping",
        HelperOp::ServiceStatus => "helper.status",
        HelperOp::ClusterList => "cluster.list",
        HelperOp::ClusterInspect { .. } => "cluster.inspect",
        HelperOp::ClusterCreate { .. } => "cluster.create",
        HelperOp::ClusterStart { .. } => "cluster.start",
        HelperOp::ClusterStop { .. } => "cluster.stop",
        HelperOp::ClusterRestart { .. } => "cluster.restart",
        HelperOp::ClusterReload { .. } => "cluster.reload",
        HelperOp::ClusterRename { .. } => "cluster.rename",
        HelperOp::ClusterDelete { .. } => "cluster.delete",
        HelperOp::ClusterReadConfig { .. } => "cluster.read_config",
        HelperOp::ClusterUpdateSafeConfig { .. } => "cluster.update_config",
        HelperOp::ClusterReadLogs { .. } => "cluster.read_logs",
    }
}

fn op_target(op: &HelperOp) -> Option<String> {
    match op {
        HelperOp::ClusterInspect { version, name }
        | HelperOp::ClusterStart { version, name }
        | HelperOp::ClusterStop { version, name, .. }
        | HelperOp::ClusterRestart { version, name }
        | HelperOp::ClusterReload { version, name }
        | HelperOp::ClusterReadConfig { version, name }
        | HelperOp::ClusterUpdateSafeConfig { version, name, .. }
        | HelperOp::ClusterReadLogs { version, name, .. }
        | HelperOp::ClusterDelete { version, name, .. } => Some(format!("{version}/{name}")),
        HelperOp::ClusterRename {
            version,
            old_name,
            new_name,
        } => Some(format!("{version}/{old_name}->{new_name}")),
        HelperOp::ClusterCreate { params } => Some(format!("{}/{}", params.version, params.name)),
        _ => None,
    }
}

/// Build an error response without audit (protocol-level failures).
pub fn protocol_error_response(request_id: Uuid, error: HelperErrorBody) -> HelperResponse {
    warn!(%request_id, code = ?error.code, message = %error.message, "protocol error");
    HelperResponse {
        request_id,
        result: HelperResult::Err { error },
    }
}
