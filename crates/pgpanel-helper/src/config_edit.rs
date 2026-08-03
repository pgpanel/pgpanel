//! Safe postgresql.conf editing with backup and atomic writes.

use pgpanel_core::validation::{is_allowlisted_config_key, validate_config_value};
use pgpanel_protocol::{ConfigSetting, HelperErrorBody, HelperErrorCode};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::cluster::{cluster_reload, cluster_restart, find_cluster, helper_err, ClusterContext};

/// Read allowlisted settings from a postgresql.conf file.
pub async fn read_allowlisted_settings(
    config_path: &Path,
) -> Result<Vec<ConfigSetting>, HelperErrorBody> {
    let text = fs::read_to_string(config_path).await.map_err(|e| {
        helper_err(
            HelperErrorCode::NotFound,
            format!("failed to read {}", config_path.display()),
            Some(e.to_string()),
        )
    })?;
    Ok(parse_allowlisted_from_conf(&text))
}

/// Parse allowlisted keys from postgresql.conf text.
pub fn parse_allowlisted_from_conf(text: &str) -> Vec<ConfigSetting> {
    let mut map = std::collections::BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = parse_conf_line(trimmed) {
            if is_allowlisted_config_key(&key) {
                map.insert(key, value);
            }
        }
    }
    map.into_iter()
        .map(|(key, value)| ConfigSetting { key, value })
        .collect()
}

fn parse_conf_line(line: &str) -> Option<(String, String)> {
    let (left, right) = line.split_once('=')?;
    let key = left.trim().to_string();
    let mut value = right.trim().to_string();
    if (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('"') && value.ends_with('"'))
    {
        value = value[1..value.len() - 1].to_string();
    }
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

/// Update allowlisted configuration keys with backup and atomic write.
pub async fn update_safe_config(
    ctx: &ClusterContext<'_>,
    version: &str,
    name: &str,
    settings: &[ConfigSetting],
    allow_restart: bool,
) -> Result<(PathBuf, bool, Vec<ConfigSetting>), HelperErrorBody> {
    if settings.is_empty() {
        return Err(helper_err(
            HelperErrorCode::InvalidInput,
            "no settings provided",
            None,
        ));
    }

    for s in settings {
        if !is_allowlisted_config_key(&s.key) {
            return Err(helper_err(
                HelperErrorCode::ConfigInvalid,
                format!("configuration key '{}' is not allowlisted", s.key),
                None,
            ));
        }
        validate_config_value(&s.key, &s.value)
            .map_err(|e| helper_err(HelperErrorCode::ConfigInvalid, e.user_message(), None))?;
    }

    find_cluster(ctx, version, name).await?;
    let config_path = PathBuf::from(format!("/etc/postgresql/{version}/{name}/postgresql.conf"));
    let original = fs::read_to_string(&config_path).await.map_err(|e| {
        helper_err(
            HelperErrorCode::NotFound,
            format!("failed to read {}", config_path.display()),
            Some(e.to_string()),
        )
    })?;

    let backup_path = backup_config(&config_path, &original).await?;
    let updated = apply_settings_to_conf(&original, settings);
    atomic_write(&config_path, &updated).await?;

    let reload_result = cluster_reload(ctx, version, name).await;
    let mut restarted = false;
    match reload_result {
        Ok(_) => {}
        Err(_) if allow_restart => {
            cluster_restart(ctx, version, name).await?;
            restarted = true;
        }
        Err(e) => {
            // Restore backup on failure.
            let _ = atomic_write(&config_path, &original).await;
            return Err(e);
        }
    }

    Ok((backup_path, restarted, settings.to_vec()))
}

async fn backup_config(config_path: &Path, content: &str) -> Result<PathBuf, HelperErrorBody> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let backup_path = config_path.with_extension(format!("conf.pgpanel-backup-{ts}"));
    atomic_write(&backup_path, content).await?;
    Ok(backup_path)
}

fn apply_settings_to_conf(original: &str, settings: &[ConfigSetting]) -> String {
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
    let mut pending: std::collections::HashMap<String, String> = settings
        .iter()
        .map(|s| (s.key.clone(), s.value.clone()))
        .collect();

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = parse_conf_line(trimmed) {
            if let Some(new_value) = pending.remove(&key) {
                let indent = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect::<String>();
                *line = format!("{indent}{key} = '{new_value}'");
            }
        }
    }

    for (key, value) in pending {
        lines.push(format!("{key} = '{value}'"));
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

async fn atomic_write(path: &Path, content: &str) -> Result<(), HelperErrorBody> {
    let parent = path.parent().ok_or_else(|| {
        helper_err(
            HelperErrorCode::Internal,
            "config path has no parent directory",
            Some(path.display().to_string()),
        )
    })?;
    fs::create_dir_all(parent).await.map_err(|e| {
        helper_err(
            HelperErrorCode::Internal,
            "failed to create config parent directory",
            Some(e.to_string()),
        )
    })?;

    let tmp_path = path.with_extension("pgpanel-tmp");
    let mut file = fs::File::create(&tmp_path).await.map_err(|e| {
        helper_err(
            HelperErrorCode::Internal,
            "failed to create temporary config file",
            Some(e.to_string()),
        )
    })?;
    file.write_all(content.as_bytes()).await.map_err(|e| {
        helper_err(
            HelperErrorCode::Internal,
            "failed to write temporary config file",
            Some(e.to_string()),
        )
    })?;
    file.sync_all().await.map_err(|e| {
        helper_err(
            HelperErrorCode::Internal,
            "failed to fsync temporary config file",
            Some(e.to_string()),
        )
    })?;
    drop(file);

    fs::rename(&tmp_path, path).await.map_err(|e| {
        helper_err(
            HelperErrorCode::Internal,
            "failed to atomically replace config file",
            Some(e.to_string()),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgpanel_core::validation::validate_listen_addresses;

    #[test]
    fn applies_and_parses_allowlisted_keys() {
        let original = "# comment\nmax_connections = 100\nwal_level = replica\n";
        let settings = vec![ConfigSetting {
            key: "max_connections".into(),
            value: "200".into(),
        }];
        let updated = apply_settings_to_conf(original, &settings);
        assert!(updated.contains("max_connections = '200'"));
        let parsed = parse_allowlisted_from_conf(&updated);
        assert!(parsed
            .iter()
            .any(|s| s.key == "max_connections" && s.value == "200"));
    }

    #[test]
    fn validate_listen_addresses_in_config_value() {
        assert!(validate_listen_addresses("127.0.0.1").is_ok());
        assert!(validate_listen_addresses("'; DROP TABLE--").is_err());
    }
}
