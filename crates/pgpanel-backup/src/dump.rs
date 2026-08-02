//! Run pg_dump / pg_restore inside the cluster container via docker exec.
//!
//! Security: never interpolate secrets into shell scripts. Passwords go via
//! `docker exec -e`, dump bytes transfer via stdin / docker cp temp files.

use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{error, info};
use uuid::Uuid;

use pgpanel_core::error::{Error, Result};
use pgpanel_core::validation::validate_safe_name;

fn assert_safe_ident(name: &str, kind: &str) -> Result<()> {
    validate_safe_name(name, kind).map_err(|e| Error::Validation(e.to_string()))
}

/// Execute pg_dump inside a running cluster container.
/// Returns raw dump bytes (custom format -Fc).
pub async fn pg_dump_custom(
    container: &str,
    database: &str,
    username: &str,
    password: &str,
    schema_only: bool,
) -> Result<Vec<u8>> {
    assert_safe_ident(database, "database")?;
    assert_safe_ident(username, "username")?;

    let mut args = vec![
        "exec".into(),
        "-e".into(),
        format!("PGPASSWORD={password}"),
        container.to_string(),
        "pg_dump".into(),
        "-U".into(),
        username.to_string(),
        "-d".into(),
        database.to_string(),
        "-Fc".into(),
        "--no-owner".into(),
        "--no-acl".into(),
    ];
    if schema_only {
        args.push("--schema-only".into());
    }

    info!(%container, %database, schema_only, "running pg_dump in container");
    let output = Command::new("docker")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec pg_dump: {e}")))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        error!(%err, "pg_dump failed");
        return Err(Error::Internal(format!("pg_dump failed: {err}")));
    }

    if output.stdout.is_empty() {
        return Err(Error::Internal("pg_dump produced empty output".into()));
    }

    Ok(output.stdout)
}

/// Extended dump with exclude schemas/tables and compression via gzip outside.
pub async fn pg_dump_custom_ex(
    container: &str,
    database: &str,
    username: &str,
    password: &str,
    schema_only: bool,
    exclude_schemas: &[&str],
    exclude_tables: &[&str],
    include_schemas: &[&str],
) -> Result<Vec<u8>> {
    assert_safe_ident(database, "database")?;
    assert_safe_ident(username, "username")?;
    for s in exclude_schemas.iter().chain(include_schemas.iter()) {
        if !s.is_empty() {
            assert_safe_ident(s, "schema")?;
        }
    }
    for t in exclude_tables {
        if t.is_empty() {
            continue;
        }
        // table may be schema.table
        for part in t.split('.') {
            assert_safe_ident(part, "table")?;
        }
    }

    let mut args = vec![
        "exec".into(),
        "-e".into(),
        format!("PGPASSWORD={password}"),
        container.to_string(),
        "pg_dump".into(),
        "-U".into(),
        username.to_string(),
        "-d".into(),
        database.to_string(),
        "-Fc".into(),
        "--no-owner".into(),
        "--no-acl".into(),
    ];
    if schema_only {
        args.push("--schema-only".into());
    }
    for s in include_schemas {
        if !s.is_empty() {
            args.push("--schema".into());
            args.push((*s).to_string());
        }
    }
    for s in exclude_schemas {
        if !s.is_empty() {
            args.push("--exclude-schema".into());
            args.push((*s).to_string());
        }
    }
    for t in exclude_tables {
        if !t.is_empty() {
            args.push("--exclude-table".into());
            args.push((*t).to_string());
        }
    }

    let output = Command::new("docker")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec pg_dump: {e}")))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!("pg_dump failed: {err}")));
    }
    if output.stdout.is_empty() {
        return Err(Error::Internal("pg_dump produced empty output".into()));
    }
    Ok(output.stdout)
}

async fn stage_dump_in_container(container: &str, dump_bytes: &[u8]) -> Result<String> {
    let remote = format!("/tmp/pgpanel_{}.dump", Uuid::new_v4());
    let host_tmp = std::env::temp_dir().join(format!("pgpanel_dump_{}.dump", Uuid::new_v4()));
    tokio::fs::write(&host_tmp, dump_bytes)
        .await
        .map_err(|e| Error::Internal(format!("write temp dump: {e}")))?;

    let cp = Command::new("docker")
        .args([
            "cp",
            &host_tmp.display().to_string(),
            &format!("{container}:{remote}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker cp: {e}")))?;

    let _ = tokio::fs::remove_file(&host_tmp).await;

    if !cp.status.success() {
        let err = String::from_utf8_lossy(&cp.stderr);
        return Err(Error::Internal(format!("docker cp failed: {err}")));
    }
    Ok(remote)
}

async fn remove_remote(container: &str, remote: &str) {
    let _ = Command::new("docker")
        .args(["exec", container, "rm", "-f", remote])
        .output()
        .await;
}

/// List objects in a custom-format dump (pg_restore -l).
pub async fn pg_restore_list(container: &str, dump_bytes: &[u8]) -> Result<String> {
    let remote = stage_dump_in_container(container, dump_bytes).await?;
    let output = Command::new("docker")
        .args(["exec", container, "pg_restore", "-l", &remote])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("pg_restore -l: {e}")))?;
    remove_remote(container, &remote).await;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!("pg_restore -l failed: {err}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Restore custom dump into database (destructive depending on flags).
pub async fn pg_restore_custom(
    container: &str,
    database: &str,
    username: &str,
    password: &str,
    dump_bytes: &[u8],
    clean: bool,
) -> Result<()> {
    assert_safe_ident(database, "database")?;
    assert_safe_ident(username, "username")?;

    let remote = stage_dump_in_container(container, dump_bytes).await?;

    let mut args = vec![
        "exec".into(),
        "-e".into(),
        format!("PGPASSWORD={password}"),
        container.to_string(),
        "pg_restore".into(),
        "-U".into(),
        username.to_string(),
        "-d".into(),
        database.to_string(),
        "--no-owner".into(),
        "--no-acl".into(),
    ];
    if clean {
        args.push("--clean".into());
        args.push("--if-exists".into());
    }
    args.push(remote.clone());

    let output = Command::new("docker")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("pg_restore: {e}")))?;

    remove_remote(container, &remote).await;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("error:") || err.contains("FATAL") {
            return Err(Error::Internal(format!("pg_restore failed: {err}")));
        }
        tracing::warn!(%err, "pg_restore finished with warnings");
    }
    Ok(())
}

/// List non-template databases inside a cluster container.
pub async fn list_databases(
    container: &str,
    username: &str,
    password: &str,
) -> Result<Vec<String>> {
    assert_safe_ident(username, "username")?;
    let output = Command::new("docker")
        .args([
            "exec",
            "-e",
            &format!("PGPASSWORD={password}"),
            container,
            "psql",
            "-U",
            username,
            "-d",
            "postgres",
            "-At",
            "-c",
            "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname;",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec psql: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!("list databases failed: {err}")));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// Stream dump bytes into container via `docker exec -i` + cat (fallback path).
#[allow(dead_code)]
pub async fn write_bytes_via_stdin(container: &str, remote_path: &str, data: &[u8]) -> Result<()> {
    let mut child = Command::new("docker")
        .args(["exec", "-i", container, "bash", "-c", &format!("cat > {remote_path}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Internal(format!("docker exec spawn: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .await
            .map_err(|e| Error::Internal(format!("stdin write: {e}")))?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| Error::Internal(format!("docker exec wait: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!("stdin transfer failed: {err}")));
    }
    Ok(())
}
