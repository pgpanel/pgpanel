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
