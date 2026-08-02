//! Run pg_dump / pg_restore inside the cluster container via docker exec.

use std::process::Stdio;
use tokio::process::Command;
use tracing::{error, info};

use pgpanel_core::error::{Error, Result};

/// Execute pg_dump inside a running cluster container.
/// Returns raw dump bytes (custom format -Fc).
pub async fn pg_dump_custom(
    container: &str,
    database: &str,
    username: &str,
    password: &str,
    schema_only: bool,
) -> Result<Vec<u8>> {
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

/// List objects in a custom-format dump (pg_restore -l).
pub async fn pg_restore_list(container: &str, dump_bytes: &[u8]) -> Result<String> {
    // Write dump into container temp, list, remove.
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(dump_bytes);

    let script = format!(
        "echo '{b64}' | base64 -d > /tmp/pgpanel_verify.dump && \
         pg_restore -l /tmp/pgpanel_verify.dump && \
         rm -f /tmp/pgpanel_verify.dump"
    );

    let output = Command::new("docker")
        .args(["exec", container, "bash", "-lc", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("pg_restore -l: {e}")))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Internal(format!("pg_restore -l failed: {err}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Restore custom dump into database (destructive to target DB objects depending on flags).
pub async fn pg_restore_custom(
    container: &str,
    database: &str,
    username: &str,
    password: &str,
    dump_bytes: &[u8],
    clean: bool,
) -> Result<()> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(dump_bytes);
    let clean_flag = if clean { "--clean --if-exists" } else { "" };
    let script = format!(
        "echo '{b64}' | base64 -d > /tmp/pgpanel_restore.dump && \
         PGPASSWORD='{password}' pg_restore -U {username} -d {database} \
         --no-owner --no-acl {clean_flag} /tmp/pgpanel_restore.dump; \
         ec=$?; rm -f /tmp/pgpanel_restore.dump; exit $ec"
    );

    let output = Command::new("docker")
        .args(["exec", container, "bash", "-lc", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| Error::Internal(format!("pg_restore: {e}")))?;

    // pg_restore may return non-zero for warnings — treat only hard failures
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        // Many non-fatal warnings; if empty dump error, fail hard
        if err.contains("error:") || err.contains("FATAL") {
            return Err(Error::Internal(format!("pg_restore failed: {err}")));
        }
        tracing::warn!(%err, "pg_restore finished with warnings");
    }
    Ok(())
}
