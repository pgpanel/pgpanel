use std::collections::HashMap;
use std::time::Duration;

use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions, StatsOptions, StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{
    HealthConfig, HostConfig, Mount, MountTypeEnum, PortBinding, ResourcesUlimits,
};
use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::volume::{CreateVolumeOptions, ListVolumesOptions};
use bollard::Docker;
use futures::StreamExt;
use tracing::{info, warn};

use pgpanel_core::config::ALLOWED_POSTGRES_IMAGES;
use pgpanel_core::error::{Error, Result};

use crate::types::{ContainerInspect, ContainerStats, PostgresContainerSpec};

/// Shell script run inside a short-lived `docker:cli` helper.
/// Near-zero downtime, SQLite-safe cutover:
/// 1) pull while old panel still serves
/// 2) stop old with short grace + start new (never two writers on panel.db)
/// 3) wait until /health is OK before exiting
const PANEL_UPDATER_SCRIPT: &str = r#"
set -eu
echo "[pgpanel-updater] target=$PGPANEL_TARGET_IMAGE version=$PGPANEL_TARGET_VERSION"
echo "[pgpanel-updater] compose_dir=$COMPOSE_DIR env_file=$ENV_FILE"

upsert_env() {
  key="$1"
  val="$2"
  file="$3"
  if [ ! -f "$file" ]; then
    echo "${key}=${val}" > "$file"
    return
  fi
  if grep -q "^${key}=" "$file"; then
    sed -i "s|^${key}=.*|${key}=${val}|" "$file"
  else
    echo "${key}=${val}" >> "$file"
  fi
}

wait_panel_healthy() {
  max="${1:-90}"
  i=0
  while [ "$i" -lt "$max" ]; do
    cid="$(docker ps -q -f label=com.docker.compose.service=panel | head -1 || true)"
    if [ -n "$cid" ]; then
      if docker exec "$cid" curl -fsS http://127.0.0.1:8080/health >/dev/null 2>&1; then
        echo "[pgpanel-updater] healthy after ${i}s"
        return 0
      fi
      # Compose healthcheck status as secondary signal
      st="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{end}}' "$cid" 2>/dev/null || true)"
      if [ "$st" = "healthy" ]; then
        echo "[pgpanel-updater] docker health=healthy after ${i}s"
        return 0
      fi
    fi
    i=$((i + 1))
    sleep 1
  done
  echo "[pgpanel-updater] health wait timed out after ${max}s"
  return 1
}

# Brief pause so apply HTTP response can flush; pull happens next while old still serves.
sleep 2

if [ -n "${ENV_FILE:-}" ] && [ -d "$(dirname "$ENV_FILE")" ]; then
  # Pin image for compose recreate — do NOT bump PGPANEL_VERSION yet
  # (env version must not claim success before the new image is healthy).
  upsert_env PGPANEL_IMAGE "$PGPANEL_TARGET_IMAGE" "$ENV_FILE" || true
  upsert_env PGPANEL_PULL_POLICY missing "$ENV_FILE" || true
  echo "[pgpanel-updater] pinned PGPANEL_IMAGE"
fi

cd "$COMPOSE_DIR"
set -a
# shellcheck disable=SC1090
[ -f "$ENV_FILE" ] && . "$ENV_FILE" || true
set +a
export PGPANEL_IMAGE="$PGPANEL_TARGET_IMAGE"
export PGPANEL_PULL_POLICY=missing

echo "[pgpanel-updater] pulling panel image (old panel still serving)"
docker pull "$PGPANEL_TARGET_IMAGE" || docker compose pull panel

echo "[pgpanel-updater] near-zero cutover: recreate panel only"
if docker compose up -d --no-deps --force-recreate --no-build --wait --wait-timeout 120 panel; then
  echo "[pgpanel-updater] compose --wait succeeded"
else
  echo "[pgpanel-updater] --wait unsupported or failed — force-recreate + health poll"
  if ! docker compose up -d --no-deps --force-recreate --no-build panel; then
    echo "[pgpanel-updater] force-recreate failed — retrying plain up"
    docker compose up -d --no-deps --no-build panel || {
      echo "[pgpanel-updater] FAILED"
      exit 1
    }
  fi
  wait_panel_healthy 90
fi

# Only after healthy: record version pins (avoids "up to date" lie on old image).
if [ -n "${ENV_FILE:-}" ] && [ -d "$(dirname "$ENV_FILE")" ]; then
  upsert_env PGPANEL_VERSION "$PGPANEL_TARGET_VERSION" "$ENV_FILE" || true
fi
INSTALL_DIR="$(dirname "$COMPOSE_DIR")"
if [ -d "$INSTALL_DIR" ]; then
  echo "$PGPANEL_TARGET_VERSION" >"$INSTALL_DIR/VERSION" || true
fi
echo "[pgpanel-updater] done"
"#;

/// Thin wrapper around Bollard with allowlisted operations only.
#[derive(Clone)]
pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn connect(docker_host: Option<&str>) -> Result<Self> {
        let docker = if let Some(host) = docker_host {
            if host.starts_with("unix://") {
                Docker::connect_with_unix(host, 120, bollard::API_DEFAULT_VERSION)
            } else {
                Docker::connect_with_http(host, 120, bollard::API_DEFAULT_VERSION)
            }
            .map_err(|e| Error::Docker(format!("connect: {e}")))?
        } else {
            Docker::connect_with_local_defaults()
                .map_err(|e| Error::Docker(format!("connect local: {e}")))?
        };
        Ok(Self { docker })
    }

    pub async fn ping(&self) -> Result<()> {
        self.docker
            .ping()
            .await
            .map_err(|e| Error::Docker(format!("ping: {e}")))?;
        Ok(())
    }

    fn assert_allowed_image(image: &str) -> Result<()> {
        if !ALLOWED_POSTGRES_IMAGES.contains(&image) {
            return Err(Error::Validation(format!(
                "image '{image}' is not allowlisted; allowed: {ALLOWED_POSTGRES_IMAGES:?}"
            )));
        }
        Ok(())
    }

    /// Pull an allowlisted PostgreSQL image if missing.
    pub async fn ensure_image(&self, image: &str) -> Result<()> {
        Self::assert_allowed_image(image)?;
        info!(%image, "ensuring PostgreSQL image is present");
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });
        let mut stream = self.docker.create_image(options, None, None);
        while let Some(item) = stream.next().await {
            item.map_err(|e| Error::Docker(format!("pull image {image}: {e}")))?;
        }
        Ok(())
    }

    /// Pull only the official PgPanel image used by the panel updater.
    pub async fn pull_panel_image(&self, image: &str) -> Result<()> {
        if !image.starts_with("ghcr.io/pgpanel/pgpanel:") {
            return Err(Error::Validation("panel image is not allowlisted".into()));
        }
        info!(%image, "pulling panel image");
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });
        let mut stream = self.docker.create_image(options, None, None);
        while let Some(item) = stream.next().await {
            item.map_err(|e| Error::Docker(format!("pull image {image}: {e}")))?;
        }
        Ok(())
    }

    async fn pull_updater_image(&self, image: &str) -> Result<()> {
        const ALLOWED: &[&str] = &["docker:27-cli", "docker:cli"];
        if !ALLOWED.contains(&image) {
            return Err(Error::Validation(format!(
                "updater image '{image}' is not allowlisted"
            )));
        }
        info!(%image, "pulling panel self-updater image");
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });
        let mut stream = self.docker.create_image(options, None, None);
        while let Some(item) = stream.next().await {
            item.map_err(|e| Error::Docker(format!("pull image {image}: {e}")))?;
        }
        Ok(())
    }

    async fn image_present(&self, image: &str) -> bool {
        self.docker.inspect_image(image).await.is_ok()
    }

    /// Schedule a panel upgrade that survives the current process dying.
    ///
    /// Critical: never `docker stop` ourselves in-process. With
    /// `restart: unless-stopped`, an explicit stop leaves the panel down
    /// forever (Caddy 502). A detached helper runs `docker compose up
    /// --force-recreate panel` after a short delay instead.
    pub async fn schedule_detached_panel_upgrade(
        &self,
        new_image: &str,
        version: &str,
    ) -> Result<()> {
        if !new_image.starts_with("ghcr.io/pgpanel/pgpanel:") {
            return Err(Error::Validation("panel image is not allowlisted".into()));
        }

        // Phase 1: pull while old panel stays up (skip redundant :latest).
        self.pull_panel_image(new_image).await?;

        let panel_id = self.find_panel_container_id().await?.ok_or_else(|| {
            Error::Docker(
                "panel container not found — run on the host: sudo pgpanel update".into(),
            )
        })?;

        let inspect = self
            .docker
            .inspect_container(&panel_id, None)
            .await
            .map_err(|e| Error::Docker(format!("inspect panel: {e}")))?;

        let labels = inspect
            .config
            .as_ref()
            .and_then(|c| c.labels.clone())
            .unwrap_or_default();

        let working_dir = labels
            .get("com.docker.compose.project.working_dir")
            .cloned()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                Error::Docker(
                    "panel is not a Compose service (missing working_dir label) — run: sudo pgpanel update"
                        .into(),
                )
            })?;

        let install_dir = std::path::Path::new(&working_dir)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| working_dir.clone());
        let env_file = format!("{install_dir}/.env");

        let mut updater_image = "docker:27-cli";
        if let Err(e) = self.pull_updater_image(updater_image).await {
            warn!(error = %e, "docker:27-cli pull failed — trying docker:cli");
            self.pull_updater_image("docker:cli").await?;
            updater_image = "docker:cli";
        } else if !self.image_present("docker:27-cli").await {
            self.pull_updater_image("docker:cli").await?;
            updater_image = "docker:cli";
        }

        let _ = self.remove_container("pgpanel-self-updater", true).await;

        let host_config = HostConfig {
            binds: Some(vec![
                "/var/run/docker.sock:/var/run/docker.sock".into(),
                format!("{working_dir}:{working_dir}"),
                format!("{install_dir}:{install_dir}"),
            ]),
            auto_remove: Some(true),
            network_mode: Some("none".into()),
            restart_policy: Some(bollard::models::RestartPolicy {
                name: Some(bollard::models::RestartPolicyNameEnum::NO),
                maximum_retry_count: None,
            }),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            ..Default::default()
        };

        let config = Config {
            image: Some(updater_image.to_string()),
            env: Some(vec![
                format!("PGPANEL_TARGET_IMAGE={new_image}"),
                format!("PGPANEL_TARGET_VERSION={version}"),
                format!("COMPOSE_DIR={working_dir}"),
                format!("ENV_FILE={env_file}"),
                format!("PANEL_CONTAINER_ID={panel_id}"),
            ]),
            cmd: Some(vec![
                "sh".into(),
                "-c".into(),
                PANEL_UPDATER_SCRIPT.to_string(),
            ]),
            host_config: Some(host_config),
            labels: Some(HashMap::from([
                ("managed-by".into(), "pgpanel".into()),
                ("pgpanel.role".into(), "self-updater".into()),
            ])),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: "pgpanel-self-updater",
            platform: None,
        };

        info!(
            image = %new_image,
            version = %version,
            compose_dir = %working_dir,
            "scheduling detached panel upgrade (compose recreate)"
        );
        let response = self
            .docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| Error::Docker(format!("create self-updater: {e}")))?;
        self.start_container(&response.id).await?;
        info!(updater_id = %response.id, "panel self-updater started");
        Ok(())
    }

    /// Pull + schedule detached compose recreate (does not stop this process).
    pub async fn upgrade_panel_image(&self, new_image: &str) -> Result<()> {
        let version = new_image
            .rsplit_once(':')
            .map(|(_, v)| v)
            .unwrap_or("latest");
        self.schedule_detached_panel_upgrade(new_image, version)
            .await
    }

    /// Image reference of the running compose `panel` service (e.g. ghcr.io/...:0.1.12).
    pub async fn running_panel_image(&self) -> Result<Option<String>> {
        let id = self.find_panel_container_id().await?;
        let Some(id) = id else {
            return Ok(None);
        };
        let inspect = self
            .docker
            .inspect_container(&id, None)
            .await
            .map_err(|e| Error::Docker(format!("inspect panel: {e}")))?;
        Ok(inspect.config.and_then(|c| c.image))
    }

    async fn find_panel_container_id(&self) -> Result<Option<String>> {
        let opts = ListContainersOptions::<String> {
            all: false,
            filters: HashMap::from([(
                "label".into(),
                vec!["com.docker.compose.service=panel".into()],
            )]),
            ..Default::default()
        };
        let mut found = self
            .docker
            .list_containers(Some(opts))
            .await
            .map_err(|e| Error::Docker(format!("list panel containers: {e}")))?;

        if found.is_empty() {
            let opts = ListContainersOptions::<String> {
                all: false,
                ..Default::default()
            };
            found = self
                .docker
                .list_containers(Some(opts))
                .await
                .map_err(|e| Error::Docker(format!("list containers: {e}")))?
                .into_iter()
                .filter(|c| {
                    c.image
                        .as_deref()
                        .map(|img| img.contains("ghcr.io/pgpanel/pgpanel"))
                        .unwrap_or(false)
                        || c.names
                            .as_ref()
                            .map(|ns| {
                                ns.iter()
                                    .any(|n| n.contains("pgpanel") && n.contains("panel"))
                            })
                            .unwrap_or(false)
                })
                .collect();
        }

        Ok(found.into_iter().next().and_then(|c| c.id))
    }

    pub async fn create_volume(&self, name: &str, labels: HashMap<String, String>) -> Result<()> {
        info!(%name, "creating Docker volume");
        let opts = CreateVolumeOptions {
            name: name.to_string(),
            driver: "local".to_string(),
            driver_opts: HashMap::new(),
            labels,
        };
        self.docker
            .create_volume(opts)
            .await
            .map_err(|e| Error::Docker(format!("create volume {name}: {e}")))?;
        Ok(())
    }

    pub async fn volume_exists(&self, name: &str) -> Result<bool> {
        let opts = Some(ListVolumesOptions::<String> {
            filters: HashMap::new(),
        });
        let list = self
            .docker
            .list_volumes(opts)
            .await
            .map_err(|e| Error::Docker(format!("list volumes: {e}")))?;
        Ok(list
            .volumes
            .unwrap_or_default()
            .iter()
            .any(|v| v.name.as_str() == name))
    }

    pub async fn remove_volume(&self, name: &str, force: bool) -> Result<()> {
        info!(%name, force, "removing Docker volume");
        use bollard::volume::RemoveVolumeOptions;
        self.docker
            .remove_volume(name, Some(RemoveVolumeOptions { force }))
            .await
            .map_err(|e| Error::Docker(format!("remove volume {name}: {e}")))?;
        Ok(())
    }

    pub async fn create_network(&self, name: &str, labels: HashMap<String, String>) -> Result<()> {
        info!(%name, "creating Docker network");
        let opts = CreateNetworkOptions {
            name: name.to_string(),
            check_duplicate: true,
            driver: "bridge".to_string(),
            internal: true, // private; no external access by default
            labels,
            ..Default::default()
        };
        self.docker
            .create_network(opts)
            .await
            .map_err(|e| Error::Docker(format!("create network {name}: {e}")))?;
        Ok(())
    }

    pub async fn network_exists(&self, name: &str) -> Result<bool> {
        let opts = Some(ListNetworksOptions::<String> {
            filters: HashMap::new(),
        });
        let list = self
            .docker
            .list_networks(opts)
            .await
            .map_err(|e| Error::Docker(format!("list networks: {e}")))?;
        Ok(list.iter().any(|n| n.name.as_deref() == Some(name)))
    }

    pub async fn remove_network(&self, name: &str) -> Result<()> {
        info!(%name, "removing Docker network");
        self.docker
            .remove_network(name)
            .await
            .map_err(|e| Error::Docker(format!("remove network {name}: {e}")))?;
        Ok(())
    }

    pub async fn create_postgres_container(&self, spec: &PostgresContainerSpec) -> Result<String> {
        Self::assert_allowed_image(&spec.image)?;

        // NanoCPUs: 1 CPU = 1e9
        let nano_cpus = (spec.cpu_limit * 1_000_000_000.0) as i64;
        let memory = (spec.memory_mb as i64) * 1024 * 1024;

        let mut port_bindings: Option<HashMap<String, Option<Vec<PortBinding>>>> = None;
        let mut exposed_ports: Option<HashMap<String, HashMap<(), ()>>> = None;

        if let Some(port) = spec.public_port {
            let mut pb = HashMap::new();
            pb.insert(
                "5432/tcp".to_string(),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.to_string()),
                }]),
            );
            port_bindings = Some(pb);
            let mut ep = HashMap::new();
            ep.insert("5432/tcp".to_string(), HashMap::new());
            exposed_ports = Some(ep);
        }

        let mut labels = HashMap::new();
        labels.insert("managed-by".to_string(), "pgpanel".to_string());
        for (k, v) in &spec.labels {
            labels.insert(k.clone(), v.clone());
        }

        let host_config = HostConfig {
            memory: Some(memory),
            nano_cpus: Some(nano_cpus),
            mounts: Some(vec![Mount {
                target: Some("/var/lib/postgresql/data".to_string()),
                source: Some(spec.volume_name.clone()),
                typ: Some(MountTypeEnum::VOLUME),
                read_only: Some(false),
                ..Default::default()
            }]),
            port_bindings,
            network_mode: Some(spec.network_name.clone()),
            restart_policy: Some(bollard::models::RestartPolicy {
                name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            ulimits: Some(vec![ResourcesUlimits {
                name: Some("nofile".to_string()),
                soft: Some(65536),
                hard: Some(65536),
            }]),
            // Security hardening
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            ..Default::default()
        };

        // Fixed environment — never accept arbitrary env from user.
        let env = vec![
            format!("POSTGRES_PASSWORD={}", spec.postgres_password),
            "POSTGRES_USER=postgres".to_string(),
            "POSTGRES_DB=postgres".to_string(),
            "PGDATA=/var/lib/postgresql/data/pgdata".to_string(),
        ];

        let healthcheck = HealthConfig {
            test: Some(vec![
                "CMD-SHELL".to_string(),
                "pg_isready -U postgres".to_string(),
            ]),
            interval: Some(5_000_000_000), // 5s in nanoseconds
            timeout: Some(3_000_000_000),
            retries: Some(10),
            start_period: Some(10_000_000_000),
            start_interval: None,
        };

        let config = Config {
            image: Some(spec.image.clone()),
            env: Some(env),
            host_config: Some(host_config),
            labels: Some(labels),
            exposed_ports,
            healthcheck: Some(healthcheck),
            hostname: Some(spec.container_name.clone()),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: &spec.container_name,
            platform: None,
        };

        info!(
            container = %spec.container_name,
            image = %spec.image,
            "creating PostgreSQL container"
        );

        let response = self
            .docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| Error::Docker(format!("create container: {e}")))?;

        Ok(response.id)
    }

    pub async fn start_container(&self, name_or_id: &str) -> Result<()> {
        info!(%name_or_id, "starting container");
        self.docker
            .start_container(name_or_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| Error::Docker(format!("start container {name_or_id}: {e}")))?;
        Ok(())
    }

    pub async fn stop_container(&self, name_or_id: &str, timeout_secs: i64) -> Result<()> {
        info!(%name_or_id, "stopping container");
        let opts = StopContainerOptions { t: timeout_secs };
        self.docker
            .stop_container(name_or_id, Some(opts))
            .await
            .map_err(|e| Error::Docker(format!("stop container {name_or_id}: {e}")))?;
        Ok(())
    }

    pub async fn remove_container(&self, name_or_id: &str, force: bool) -> Result<()> {
        info!(%name_or_id, force, "removing container");
        let opts = RemoveContainerOptions {
            v: false, // never auto-remove volumes
            force,
            link: false,
        };
        self.docker
            .remove_container(name_or_id, Some(opts))
            .await
            .map_err(|e| Error::Docker(format!("remove container {name_or_id}: {e}")))?;
        Ok(())
    }

    pub async fn inspect_container(&self, name_or_id: &str) -> Result<ContainerInspect> {
        let info = self
            .docker
            .inspect_container(name_or_id, None)
            .await
            .map_err(|e| Error::Docker(format!("inspect {name_or_id}: {e}")))?;

        let state = info.state.unwrap_or_default();
        let health = state
            .health
            .and_then(|h| h.status)
            .map(|s| format!("{s:?}").to_lowercase());

        Ok(ContainerInspect {
            id: info.id.unwrap_or_default(),
            name: info.name.unwrap_or_default(),
            running: state.running.unwrap_or(false),
            health,
            started_at: state.started_at,
        })
    }

    pub async fn container_exists(&self, name: &str) -> Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("name".to_string(), vec![format!("^{name}$")]);
        let opts = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };
        let list = self
            .docker
            .list_containers(Some(opts))
            .await
            .map_err(|e| Error::Docker(format!("list containers: {e}")))?;
        Ok(!list.is_empty())
    }

    /// Wait until container health is healthy or timeout.
    pub async fn wait_healthy(&self, name_or_id: &str, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(Error::Docker(format!(
                    "timeout waiting for container {name_or_id} to become healthy"
                )));
            }
            match self.inspect_container(name_or_id).await {
                Ok(info) if info.running => {
                    let h = info.health.as_deref().unwrap_or("");
                    if h.contains("healthy") {
                        return Ok(());
                    }
                    if h.contains("unhealthy") {
                        // keep waiting a bit — postgres may still be initializing
                    }
                }
                Ok(_) => {}
                Err(e) => warn!(error = %e, "inspect during health wait"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    pub async fn container_stats(&self, name_or_id: &str) -> Result<ContainerStats> {
        let mut stream = self.docker.stats(
            name_or_id,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        let stats = stream
            .next()
            .await
            .ok_or_else(|| Error::Docker("no stats returned".into()))?
            .map_err(|e| Error::Docker(format!("stats: {e}")))?;

        let cpu_delta = stats.cpu_stats.cpu_usage.total_usage as f64
            - stats.precpu_stats.cpu_usage.total_usage as f64;
        let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0) as f64
            - stats.precpu_stats.system_cpu_usage.unwrap_or(0) as f64;
        let online_cpus = stats.cpu_stats.online_cpus.unwrap_or(1) as f64;
        let cpu_percent = if system_delta > 0.0 {
            (cpu_delta / system_delta) * online_cpus * 100.0
        } else {
            0.0
        };

        let mem_usage = stats.memory_stats.usage.unwrap_or(0) as f64 / (1024.0 * 1024.0);
        let mem_limit = stats.memory_stats.limit.unwrap_or(0) as f64 / (1024.0 * 1024.0);

        Ok(ContainerStats {
            cpu_percent,
            memory_usage_mb: mem_usage,
            memory_limit_mb: mem_limit,
        })
    }

    /// Attach a container to an additional Docker network (dual-homing).
    ///
    /// Used so PG clusters (on a private per-cluster network) also join
    /// `pgpanel_database_management`, where the panel and Databasus resolve
    /// them by container name. Already-connected is treated as success.
    pub async fn connect_network(&self, network_name: &str, container_name: &str) -> Result<()> {
        use bollard::models::EndpointSettings;
        use bollard::network::ConnectNetworkOptions;

        info!(%network_name, %container_name, "connecting container to network");
        let config = ConnectNetworkOptions {
            container: container_name,
            endpoint_config: EndpointSettings {
                aliases: Some(vec![container_name.to_string()]),
                ..Default::default()
            },
        };
        match self.docker.connect_network(network_name, config).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                // Idempotent: already on network
                if msg.contains("already exists")
                    || msg.contains("already connected")
                    || msg.contains("endpoint with name")
                {
                    info!(%network_name, %container_name, "container already on network");
                    return Ok(());
                }
                Err(Error::Docker(format!(
                    "connect {container_name} to {network_name}: {e}"
                )))
            }
        }
    }
}
