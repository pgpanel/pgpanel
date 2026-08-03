//! PostgreSQL cluster operations via postgresql-common binaries.

use pgpanel_core::config::{Config, PostgresBinaries};
use pgpanel_core::error::CoreError;
use pgpanel_core::validation::{
    validate_cluster_name, validate_delete_confirmation, validate_encoding,
    validate_listen_addresses, validate_locale, validate_path_under_roots, validate_pg_version,
    validate_port,
};
use pgpanel_protocol::{
    ClusterCreateParams, ClusterDetail, ClusterSummary, ConfigSetting, HelperErrorBody,
    HelperErrorCode,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::debug;

use crate::config_edit;

/// Output from an external command.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    /// Exit code if process exited.
    pub status: Option<i32>,
    /// Captured stdout (UTF-8 lossy normalized at use sites).
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
}

impl CmdOutput {
    /// stdout as lossy UTF-8 string.
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// stderr as lossy UTF-8 string.
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Whether the process exited successfully.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

/// Shared helper execution context.
pub struct ClusterContext<'a> {
    /// Application configuration.
    pub config: &'a Config,
}

impl<'a> ClusterContext<'a> {
    /// Create a new context.
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    fn binaries(&self) -> &PostgresBinaries {
        &self.config.postgres.binaries
    }
}

/// Run an absolute-path executable with discrete arguments (never a shell).
pub async fn run_command(
    program: &Path,
    args: &[&str],
    timeout_dur: Duration,
) -> Result<CmdOutput, HelperErrorBody> {
    if !program.is_absolute() {
        return Err(helper_err(
            HelperErrorCode::Internal,
            "executable path must be absolute",
            Some(program.display().to_string()),
        ));
    }
    let arg_strings: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    debug!(
        program = %program.display(),
        ?arg_strings,
        timeout_secs = timeout_dur.as_secs(),
        "executing command"
    );

    let mut child = Command::new(program);
    child.args(args);
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());

    let result = timeout(timeout_dur, child.output())
        .await
        .map_err(|_| {
            helper_err(
                HelperErrorCode::Timeout,
                "command timed out",
                Some(program.display().to_string()),
            )
        })?
        .map_err(|e| map_io_error("failed to spawn command", e))?;

    Ok(CmdOutput {
        status: result.status.code(),
        stdout: result.stdout,
        stderr: result.stderr,
    })
}

/// List all clusters via `pg_lsclusters`.
pub async fn cluster_list(
    ctx: &ClusterContext<'_>,
) -> Result<Vec<ClusterSummary>, HelperErrorBody> {
    let out = run_command(
        &ctx.binaries().pg_lsclusters,
        &["--json"],
        Duration::from_secs(ctx.config.timeouts.helper_default_secs),
    )
    .await?;

    if out.success() {
        return parse_pg_lsclusters_json(&out.stdout_str());
    }

    // Fallback to tabular output when --json is unsupported.
    let out = run_command(
        &ctx.binaries().pg_lsclusters,
        &[],
        Duration::from_secs(ctx.config.timeouts.helper_default_secs),
    )
    .await?;

    if !out.success() {
        return Err(command_failed(
            &ctx.binaries().pg_lsclusters,
            &out,
            "pg_lsclusters failed",
        ));
    }
    parse_pg_lsclusters_tabular(&out.stdout_str())
}

/// Find a single cluster by version and name.
pub async fn find_cluster(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<ClusterSummary, HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    let clusters = cluster_list(ctx).await?;
    clusters
        .into_iter()
        .find(|c| c.version == version && c.name == name)
        .ok_or_else(|| {
            helper_err(
                HelperErrorCode::NotFound,
                format!("cluster {version}/{name} not found"),
                None,
            )
        })
}

/// Inspect a cluster with optional runtime statistics.
pub async fn cluster_inspect(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<ClusterDetail, HelperErrorBody> {
    let summary = find_cluster(ctx, version, name).await?;
    let config_directory = config_directory_path(version, name);
    let status_out = run_command(
        &ctx.binaries().pg_ctlcluster,
        &["status", version, name],
        Duration::from_secs(ctx.config.timeouts.cluster_control_secs),
    )
    .await?;

    let status_text = status_out.stdout_str();
    let postmaster_pid = extract_pid(&status_text);
    let data_dir_bytes = dir_size_bytes(&summary.data_directory).await.ok();

    let mut detail = ClusterDetail {
        summary,
        config_directory: config_directory.display().to_string(),
        postmaster_pid,
        uptime_secs: None,
        connection_count: None,
        database_count: None,
        total_size_bytes: data_dir_bytes,
        data_dir_bytes,
        data_checksums: None,
        wal_level: None,
        ssl: None,
        listen_addresses: None,
        max_connections: None,
        replication_slot_count: None,
        last_checkpoint: None,
    };

    if let Ok(settings) =
        config_edit::read_allowlisted_settings(&config_directory.join("postgresql.conf")).await
    {
        for s in settings {
            match s.key.as_str() {
                "wal_level" => detail.wal_level = Some(s.value),
                "ssl" => {
                    detail.ssl = Some(matches!(s.value.as_str(), "on" | "true" | "1"));
                }
                "listen_addresses" => detail.listen_addresses = Some(s.value),
                "max_connections" => {
                    detail.max_connections = s.value.parse().ok();
                }
                _ => {}
            }
        }
    }

    Ok(detail)
}

/// Create a new PostgreSQL cluster.
pub async fn cluster_create(
    ctx: &ClusterContext<'_>,
    params: &ClusterCreateParams,
) -> Result<ClusterSummary, HelperErrorBody> {
    validate_cluster_pair(&params.version, &params.name)?;
    validate_pg_version(&params.version).map_err(map_core_err)?;
    validate_port(params.port, false).map_err(map_core_err)?;
    validate_encoding(&params.encoding).map_err(map_core_err)?;
    validate_locale(&params.locale).map_err(map_core_err)?;
    validate_listen_addresses(&params.listen_addresses).map_err(map_core_err)?;

    if !ctx
        .config
        .postgres
        .allowed_versions
        .contains(&params.version)
    {
        return Err(helper_err(
            HelperErrorCode::VersionNotInstalled,
            format!("PostgreSQL {} is not allowed by policy", params.version),
            None,
        ));
    }

    let initdb_path = PathBuf::from(format!("/usr/lib/postgresql/{}/bin/initdb", params.version));
    if tokio::fs::metadata(&initdb_path).await.is_err() {
        return Err(helper_err(
            HelperErrorCode::VersionNotInstalled,
            format!(
                "PostgreSQL {} server binaries are not installed; install postgresql-{} first",
                params.version, params.version
            ),
            Some(format!("missing executable: {}", initdb_path.display())),
        ));
    }

    if find_cluster(ctx, &params.version, &params.name)
        .await
        .is_ok()
    {
        return Err(helper_err(
            HelperErrorCode::AlreadyExists,
            format!("cluster {}/{} already exists", params.version, params.name),
            None,
        ));
    }

    let mut args: Vec<String> = vec![
        params.version.clone(),
        params.name.clone(),
        "--port".into(),
        params.port.to_string(),
        "--locale".into(),
        params.locale.clone(),
        "--encoding".into(),
        params.encoding.clone(),
    ];

    if let Some(ref data_dir) = params.data_directory {
        let validated =
            validate_path_under_roots(data_dir, &ctx.config.postgres.data_directory_roots)
                .map_err(map_core_err)?;
        args.push("-d".into());
        args.push(validated.display().to_string());
    }

    if params.start {
        args.push("--start".into());
    }

    if params.data_checksums {
        args.push("--".into());
        args.push("--data-checksums".into());
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_command(
        &ctx.binaries().pg_createcluster,
        &arg_refs,
        Duration::from_secs(ctx.config.timeouts.cluster_create_secs),
    )
    .await?;

    if !out.success() {
        return Err(classify_create_failure(&out, params));
    }

    // Apply listen_addresses via safe config update.
    let listen_settings = vec![ConfigSetting {
        key: "listen_addresses".into(),
        value: params.listen_addresses.clone(),
    }];
    let _ = config_edit::update_safe_config(
        ctx,
        &params.version,
        &params.name,
        &listen_settings,
        false,
    )
    .await;

    find_cluster(ctx, &params.version, &params.name).await
}

/// Start a cluster.
pub async fn cluster_start(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<String, HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    find_cluster(ctx, version, name).await?;
    cluster_control(ctx, version, name, &["start"], "start").await
}

/// Stop a cluster.
pub async fn cluster_stop(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
    force: bool,
) -> Result<String, HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    find_cluster(ctx, version, name).await?;
    if force {
        cluster_control(ctx, version, name, &["stop", "--mode", "immediate"], "stop").await
    } else {
        cluster_control(ctx, version, name, &["stop"], "stop").await
    }
}

/// Restart a cluster.
pub async fn cluster_restart(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<String, HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    find_cluster(ctx, version, name).await?;
    cluster_control(ctx, version, name, &["restart"], "restart").await
}

/// Reload a cluster configuration.
pub async fn cluster_reload(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<String, HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    find_cluster(ctx, version, name).await?;
    cluster_control(ctx, version, name, &["reload"], "reload").await
}

async fn cluster_control(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
    action_args: &[&str],
    action_name: &str,
) -> Result<String, HelperErrorBody> {
    let mut args = vec![action_args[0], version, name];
    if action_args.len() > 1 {
        args.extend_from_slice(&action_args[1..]);
    }
    let out = run_command(
        &ctx.binaries().pg_ctlcluster,
        &args,
        Duration::from_secs(ctx.config.timeouts.cluster_control_secs),
    )
    .await?;
    if !out.success() {
        return Err(command_failed(
            &ctx.binaries().pg_ctlcluster,
            &out,
            &format!("pg_ctlcluster {action_name} failed"),
        ));
    }
    let status = cluster_status_string(ctx, version, name).await.ok();
    Ok(status.unwrap_or_else(|| "unknown".into()))
}

async fn cluster_status_string(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<String, HelperErrorBody> {
    let cluster = find_cluster(ctx, version, name).await?;
    Ok(cluster.status)
}

/// Rename a cluster.
pub async fn cluster_rename(
    ctx: &ClusterContext<'_>,
    version: &str,
    old_name: &str,
    new_name: &str,
) -> Result<(), HelperErrorBody> {
    validate_cluster_pair(version, old_name)?;
    validate_cluster_name(new_name).map_err(map_core_err)?;
    find_cluster(ctx, version, old_name).await?;
    if find_cluster(ctx, version, new_name).await.is_ok() {
        return Err(helper_err(
            HelperErrorCode::AlreadyExists,
            format!("cluster {version}/{new_name} already exists"),
            None,
        ));
    }

    let out = run_command(
        &ctx.binaries().pg_renamecluster,
        &[version, old_name, new_name],
        Duration::from_secs(ctx.config.timeouts.cluster_control_secs),
    )
    .await?;

    if !out.success() {
        return Err(command_failed(
            &ctx.binaries().pg_renamecluster,
            &out,
            "pg_renamecluster failed",
        ));
    }
    Ok(())
}

/// Delete a cluster after confirmation and existence check.
pub async fn cluster_delete(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
    confirmation: &str,
    stop_first: bool,
) -> Result<(String, String), HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    validate_delete_confirmation(name, confirmation)
        .map_err(|e| helper_err(HelperErrorCode::ConfirmationFailed, e.user_message(), None))?;

    let cluster = find_cluster(ctx, version, name).await?;
    let data_directory = cluster.data_directory.clone();
    let config_directory = config_directory_path(version, name).display().to_string();

    let mut args = Vec::new();
    if stop_first {
        args.push("--stop");
    }
    args.push(version);
    args.push(name);

    let out = run_command(
        &ctx.binaries().pg_dropcluster,
        &args,
        Duration::from_secs(ctx.config.timeouts.cluster_delete_secs),
    )
    .await?;

    if !out.success() {
        return Err(command_failed(
            &ctx.binaries().pg_dropcluster,
            &out,
            "pg_dropcluster failed",
        ));
    }

    Ok((data_directory, config_directory))
}

/// Read allowlisted configuration for a cluster.
pub async fn cluster_read_config(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
) -> Result<(PathBuf, Vec<ConfigSetting>), HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    find_cluster(ctx, version, name).await?;
    let config_path = config_directory_path(version, name).join("postgresql.conf");
    let settings = config_edit::read_allowlisted_settings(&config_path).await?;
    Ok((config_path, settings))
}

/// Read tail of cluster log file reported by pg_lsclusters.
pub async fn cluster_read_logs(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
    max_lines: u32,
    level_filter: Option<&str>,
    search: Option<&str>,
) -> Result<(String, Vec<String>, bool), HelperErrorBody> {
    validate_cluster_pair(version, name)?;
    let cluster = find_cluster(ctx, version, name).await?;
    let log_path = cluster.log_file;
    if log_path.is_empty() {
        return Err(helper_err(
            HelperErrorCode::NotFound,
            "cluster has no log file path",
            None,
        ));
    }
    let path = PathBuf::from(&log_path);
    if !path.is_absolute() {
        return Err(helper_err(
            HelperErrorCode::Internal,
            "log path from pg_lsclusters is not absolute",
            Some(log_path.clone()),
        ));
    }

    let content = tokio::time::timeout(
        Duration::from_secs(ctx.config.timeouts.log_read_secs),
        tokio::fs::read_to_string(&path),
    )
    .await
    .map_err(|_| {
        helper_err(
            HelperErrorCode::Timeout,
            "reading log file timed out",
            Some(log_path.clone()),
        )
    })?
    .map_err(|e| map_io_error("failed to read log file", e))?;

    let max = max_lines.clamp(1, 10_000) as usize;
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let truncated = lines.len() > max;
    if truncated {
        lines = lines.split_off(lines.len().saturating_sub(max));
    }

    if let Some(level) = level_filter {
        let level_upper = level.to_uppercase();
        lines.retain(|l| l.to_uppercase().contains(&level_upper));
    }
    if let Some(needle) = search {
        if !needle.is_empty() {
            lines.retain(|l| l.contains(needle));
        }
    }

    Ok((log_path, lines, truncated))
}

fn validate_cluster_pair(version: &str, name: &str) -> Result<(), HelperErrorBody> {
    validate_pg_version(version).map_err(map_core_err)?;
    validate_cluster_name(name).map_err(map_core_err)?;
    Ok(())
}

fn config_directory_path(version: &str, name: &str) -> PathBuf {
    PathBuf::from(format!("/etc/postgresql/{version}/{name}"))
}

fn extract_pid(status_text: &str) -> Option<u32> {
    for line in status_text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("pid") {
            for token in line.split_whitespace() {
                if let Ok(pid) = token.trim_matches(|c: char| !c.is_ascii_digit()).parse() {
                    return Some(pid);
                }
            }
        }
    }
    None
}

async fn dir_size_bytes(path: &str) -> Result<u64, std::io::Error> {
    let mut total = 0u64;
    let mut stack = vec![PathBuf::from(path)];
    while let Some(p) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&p).await?;
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

/// Parse `pg_lsclusters --json` output.
pub fn parse_pg_lsclusters_json(text: &str) -> Result<Vec<ClusterSummary>, HelperErrorBody> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
        return rows
            .into_iter()
            .filter_map(|v| json_row_to_summary(&v))
            .collect::<Result<Vec<_>, _>>();
    }

    #[derive(Deserialize)]
    struct JsonWrapper {
        clusters: Option<Vec<serde_json::Value>>,
    }
    if let Ok(wrapper) = serde_json::from_str::<JsonWrapper>(trimmed) {
        if let Some(rows) = wrapper.clusters {
            return rows
                .into_iter()
                .filter_map(|v| json_row_to_summary(&v))
                .collect::<Result<Vec<_>, _>>();
        }
    }

    Err(helper_err(
        HelperErrorCode::CommandFailed,
        "failed to parse pg_lsclusters JSON output",
        Some(trimmed.chars().take(200).collect()),
    ))
}

fn json_row_to_summary(v: &serde_json::Value) -> Option<Result<ClusterSummary, HelperErrorBody>> {
    let version = field_str(v, &["version", "ver", "Version"])?;
    let name = field_str(v, &["name", "cluster", "Cluster"])?;
    let port = field_u16(v, &["port", "Port"])?;
    let status = field_str(v, &["status", "Status"]).unwrap_or_else(|| "unknown".into());
    let owner = field_str(v, &["owner", "Owner"]).unwrap_or_else(|| "postgres".into());
    let data_directory = field_str(
        v,
        &["data_directory", "data_dir", "Data directory", "datadir"],
    )?;
    let log_file = field_str(v, &["log_file", "log", "Log file", "logfile"]).unwrap_or_default();

    Some(Ok(ClusterSummary {
        version,
        name,
        port,
        status,
        owner,
        data_directory,
        log_file,
    }))
}

fn field_str(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(val) = v.get(*key) {
            if let Some(s) = val.as_str() {
                return Some(s.to_string());
            }
            if val.is_number() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn field_u16(v: &serde_json::Value, keys: &[&str]) -> Option<u16> {
    for key in keys {
        if let Some(val) = v.get(*key) {
            if let Some(n) = val.as_u64() {
                return u16::try_from(n).ok();
            }
            if let Some(s) = val.as_str() {
                return s.parse().ok();
            }
        }
    }
    None
}

/// Parse tabular `pg_lsclusters` output (no --json).
pub fn parse_pg_lsclusters_tabular(text: &str) -> Result<Vec<ClusterSummary>, HelperErrorBody> {
    let mut clusters = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("Ver ") || line.starts_with('-') {
            continue;
        }
        if let Some(summary) = parse_tabular_line(line) {
            clusters.push(summary?);
        }
    }
    Ok(clusters)
}

fn parse_tabular_line(line: &str) -> Option<Result<ClusterSummary, HelperErrorBody>> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 7 {
        return None;
    }
    let version = parts[0].to_string();
    let name = parts[1].to_string();
    let port: u16 = parts[2].parse().ok()?;
    let status = parts[3].to_string();
    let owner = parts[4].to_string();

    // Remaining tokens may include absolute paths; collect path-like tokens.
    let remainder = parts[5..].join(" ");
    let paths: Vec<&str> = remainder
        .split_whitespace()
        .filter(|p| p.starts_with('/'))
        .collect();

    let (data_directory, log_file) = if paths.len() >= 2 {
        (paths[0].to_string(), paths[1].to_string())
    } else if paths.len() == 1 {
        (paths[0].to_string(), String::new())
    } else {
        // Fallback: split remainder on double-space boundaries.
        let chunks: Vec<&str> = line
            .split("  ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if chunks.len() >= 3 {
            let data_directory = chunks[chunks.len() - 2].to_string();
            let log_file = chunks[chunks.len() - 1].to_string();
            (data_directory, log_file)
        } else {
            return Some(Err(helper_err(
                HelperErrorCode::CommandFailed,
                "failed to parse pg_lsclusters tabular line",
                Some(line.to_string()),
            )));
        }
    };

    Some(Ok(ClusterSummary {
        version,
        name,
        port,
        status,
        owner,
        data_directory,
        log_file,
    }))
}

pub(crate) fn helper_err(
    code: HelperErrorCode,
    message: impl Into<String>,
    details: Option<String>,
) -> HelperErrorBody {
    HelperErrorBody {
        code,
        message: message.into(),
        details,
    }
}

pub(crate) fn map_core_err(err: CoreError) -> HelperErrorBody {
    let code = match &err {
        CoreError::InvalidInput(_) => HelperErrorCode::InvalidInput,
        CoreError::NotFound(_) => HelperErrorCode::NotFound,
        CoreError::Forbidden(_) => HelperErrorCode::Forbidden,
        CoreError::Config(_) => HelperErrorCode::ConfigInvalid,
        CoreError::Io(_) => HelperErrorCode::Internal,
        _ => HelperErrorCode::Internal,
    };
    HelperErrorBody {
        code,
        message: err.user_message(),
        details: None,
    }
}

fn map_io_error(context: &str, err: std::io::Error) -> HelperErrorBody {
    helper_err(HelperErrorCode::Internal, context, Some(err.to_string()))
}

fn command_failed(_program: &Path, out: &CmdOutput, message: &str) -> HelperErrorBody {
    let details = command_failure_details(out);
    helper_err(HelperErrorCode::CommandFailed, message, details)
}

/// Sanitize command stderr/stdout for authenticated admin diagnostics.
pub(crate) fn sanitize_command_diagnostic(raw: &str) -> String {
    const MAX_LEN: usize = 500;

    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
        if out.len() >= MAX_LEN {
            out.push('…');
            break;
        }
    }

    let trimmed = out.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    redact_sensitive_tokens(trimmed)
}

fn redact_sensitive_tokens(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("password=")
            || lower.contains("pgpassword")
            || lower.contains("secret=")
            || lower.contains("token=")
        {
            lines.push("[redacted sensitive output]".to_string());
        } else {
            lines.push(redact_home_paths(line));
        }
    }
    lines.join("\n")
}

fn redact_home_paths(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(idx) = rest.find("/home/") {
        out.push_str(&rest[..idx]);
        out.push_str("[home]/");
        rest = &rest[idx + "/home/".len()..];
        if let Some(slash) = rest.find('/') {
            rest = &rest[slash + 1..];
        } else {
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

fn command_failure_details(out: &CmdOutput) -> Option<String> {
    let stderr = sanitize_command_diagnostic(&out.stderr_str());
    if !stderr.is_empty() {
        return Some(stderr);
    }
    out.status
        .map(|code| format!("process exited with code {code}"))
}

fn classify_create_failure(out: &CmdOutput, params: &ClusterCreateParams) -> HelperErrorBody {
    let stderr = out.stderr_str();
    let details = command_failure_details(out);
    let lower = stderr.to_ascii_lowercase();
    let summary = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(sanitize_command_diagnostic)
        .filter(|line| !line.is_empty());

    if lower.contains("already exists") || lower.contains("cluster already exists") {
        return helper_err(
            HelperErrorCode::AlreadyExists,
            format!("cluster {}/{} already exists", params.version, params.name),
            details,
        );
    }
    if lower.contains("port") && (lower.contains("in use") || lower.contains("already")) {
        return helper_err(
            HelperErrorCode::PortInUse,
            format!("port {} is already in use", params.port),
            details,
        );
    }
    if lower.contains("not installed")
        || lower.contains("unknown version")
        || lower.contains("no such version")
        || lower.contains("could not find")
    {
        return helper_err(
            HelperErrorCode::VersionNotInstalled,
            format!("PostgreSQL {} is not installed", params.version),
            details,
        );
    }
    if lower.contains("no space left") || lower.contains("disk full") {
        return helper_err(
            HelperErrorCode::InsufficientDisk,
            "insufficient disk space to create cluster",
            details,
        );
    }
    if lower.contains("invalid locale") || (lower.contains("locale") && lower.contains("not found"))
    {
        return helper_err(
            HelperErrorCode::InvalidInput,
            format!("invalid locale {:?}", params.locale),
            details,
        );
    }
    if lower.contains("invalid encoding")
        || (lower.contains("encoding") && lower.contains("not supported"))
    {
        return helper_err(
            HelperErrorCode::InvalidInput,
            format!("invalid encoding {:?}", params.encoding),
            details,
        );
    }

    let message = summary.unwrap_or_else(|| "pg_createcluster failed".into());
    helper_err(HelperErrorCode::CommandFailed, message, details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_array() {
        let json = r#"[
            {
                "version": "17",
                "name": "main",
                "port": 5432,
                "status": "online",
                "owner": "postgres",
                "data_directory": "/var/lib/postgresql/17/main",
                "log_file": "/var/log/postgresql/postgresql-17-main.log"
            }
        ]"#;
        let clusters = parse_pg_lsclusters_json(json).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].version, "17");
        assert_eq!(clusters[0].name, "main");
        assert_eq!(clusters[0].port, 5432);
    }

    #[test]
    fn parses_json_wrapper_object() {
        let json = r#"{"clusters":[{"cluster":"main","version":"16","port":"5433","status":"down","owner":"postgres","data_directory":"/var/lib/postgresql/16/main","log_file":"/var/log/postgresql/postgresql-16-main.log"}]}"#;
        let clusters = parse_pg_lsclusters_json(json).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].name, "main");
        assert_eq!(clusters[0].port, 5433);
    }

    #[test]
    fn parses_tabular_output() {
        let text = "Ver Cluster Port Status Owner    Data directory              Log file\n\
                    17  main    5432 online postgres /var/lib/postgresql/17/main /var/log/postgresql/postgresql-17-main.log\n";
        let clusters = parse_pg_lsclusters_tabular(text).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].version, "17");
        assert_eq!(clusters[0].name, "main");
        assert_eq!(clusters[0].data_directory, "/var/lib/postgresql/17/main");
    }

    #[test]
    fn rejects_invalid_cluster_name_before_exec() {
        let err = validate_cluster_pair("17", "bad-name").unwrap_err();
        assert_eq!(err.code, HelperErrorCode::InvalidInput);
        let err = validate_cluster_pair("99", "main").unwrap_err();
        assert_eq!(err.code, HelperErrorCode::InvalidInput);
        let err = validate_cluster_pair("17", "x").unwrap_err();
        assert_eq!(err.code, HelperErrorCode::InvalidInput);
    }

    #[test]
    fn delete_confirmation_phrase() {
        assert!(cluster_delete_validate_confirmation("main", "DELETE main")
            .unwrap()
            .is_ok());
        let err = cluster_delete_validate_confirmation("main", "delete main")
            .unwrap()
            .unwrap_err();
        assert_eq!(err.code, HelperErrorCode::ConfirmationFailed);
    }

    fn cluster_delete_validate_confirmation(
        name: &str,
        confirmation: &str,
    ) -> Result<Result<(), HelperErrorBody>, HelperErrorBody> {
        validate_cluster_pair("17", name)?;
        let result = validate_delete_confirmation(name, confirmation)
            .map_err(|e| helper_err(HelperErrorCode::ConfirmationFailed, e.user_message(), None));
        Ok(result.map(|_| ()))
    }

    #[test]
    fn sanitize_command_diagnostic_strips_control_chars_and_caps_length() {
        let raw = format!("bad\x07news\nsecond line{}", "x".repeat(600));
        let sanitized = sanitize_command_diagnostic(&raw);
        assert!(!sanitized.contains('\x07'));
        assert!(sanitized.contains("bad"));
        assert!(sanitized.contains("second line"));
        assert!(sanitized.chars().count() <= 501);
        assert!(sanitized.ends_with('…'));
    }

    #[test]
    fn sanitize_command_diagnostic_redacts_sensitive_tokens() {
        let sanitized = sanitize_command_diagnostic("failed: password=sekret\nport in use");
        assert!(sanitized.contains("[redacted sensitive output]"));
        assert!(sanitized.contains("port in use"));
    }

    #[test]
    fn sanitize_command_diagnostic_redacts_home_paths() {
        let sanitized =
            sanitize_command_diagnostic("cannot access /home/alice/.pg/data: permission denied");
        assert!(sanitized.contains("[home]/"));
        assert!(!sanitized.contains("/home/alice"));
    }

    #[test]
    fn classify_create_failure_maps_port_in_use() {
        let out = CmdOutput {
            status: Some(1),
            stdout: Vec::new(),
            stderr: b"Error: port 5432 already in use\n".to_vec(),
        };
        let err = classify_create_failure(
            &out,
            &ClusterCreateParams {
                version: "17".into(),
                name: "main".into(),
                display_name: None,
                port: 5432,
                encoding: "UTF8".into(),
                locale: "C.UTF-8".into(),
                data_checksums: false,
                start: false,
                listen_addresses: "127.0.0.1".into(),
                data_directory: None,
            },
        );
        assert_eq!(err.code, HelperErrorCode::PortInUse);
        assert!(err.message.contains("5432"));
        assert!(err.details.as_ref().unwrap().contains("already in use"));
    }

    #[test]
    fn classify_create_failure_uses_stderr_summary_for_command_failed() {
        let out = CmdOutput {
            status: Some(1),
            stdout: Vec::new(),
            stderr: b"initdb: could not create directory: Permission denied\n".to_vec(),
        };
        let err = classify_create_failure(
            &out,
            &ClusterCreateParams {
                version: "17".into(),
                name: "main".into(),
                display_name: None,
                port: 5433,
                encoding: "UTF8".into(),
                locale: "C.UTF-8".into(),
                data_checksums: false,
                start: false,
                listen_addresses: "127.0.0.1".into(),
                data_directory: None,
            },
        );
        assert_eq!(err.code, HelperErrorCode::CommandFailed);
        assert!(err.message.contains("Permission denied"));
        assert!(err.details.as_ref().unwrap().contains("Permission denied"));
    }

    #[test]
    fn data_checksums_args_use_initdb_separator() {
        let mut args: Vec<String> =
            vec!["17".into(), "main".into(), "--port".into(), "5433".into()];
        args.push("--".into());
        args.push("--data-checksums".into());
        assert_eq!(args.last().map(String::as_str), Some("--data-checksums"));
        assert_eq!(args[args.len() - 2].as_str(), "--");
    }
}
