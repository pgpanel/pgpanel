use std::collections::HashMap;
use std::time::Duration;

use tracing::{error, info};
use uuid::Uuid;

use pgpanel_core::config::Config;
use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::CreateClusterRequest;
use pgpanel_core::validation::{
    slugify, validate_cpu_limit, validate_display_name, validate_memory_mb, validate_public_port,
    validate_safe_name, validate_storage_gb,
};

use crate::client::DockerClient;
use crate::types::{PostgresContainerSpec, ResourceNames};

/// Resources created during provisioning (for partial-failure tracking).
#[derive(Debug, Clone, Default)]
pub struct ProvisionedResources {
    pub volume_created: bool,
    pub network_created: bool,
    pub container_created: bool,
    pub container_id: Option<String>,
    pub container_started: bool,
}

/// High-level cluster Docker provisioning.
#[derive(Clone)]
pub struct ClusterProvisioner {
    docker: DockerClient,
    config: Config,
}

impl ClusterProvisioner {
    pub fn new(docker: DockerClient, config: Config) -> Self {
        Self { docker, config }
    }

    pub fn docker(&self) -> &DockerClient {
        &self.docker
    }

    pub fn resource_names(&self, slug: &str) -> ResourceNames {
        ResourceNames {
            container_name: format!("{}{}", self.config.cluster_container_prefix, slug),
            volume_name: format!("{}{}", self.config.cluster_volume_prefix, slug),
            network_name: format!("{}{}", self.config.cluster_network_prefix, slug),
            internal_hostname: format!("{}{}", self.config.cluster_container_prefix, slug),
        }
    }

    pub fn validate_create_request(req: &CreateClusterRequest) -> Result<String> {
        validate_display_name(&req.name)?;
        let slug = slugify(&req.name);
        validate_safe_name(&slug, "cluster slug")?;
        Config::postgres_image(&req.postgres_version)?;
        validate_cpu_limit(req.cpu_limit)?;
        validate_memory_mb(req.memory_mb)?;
        validate_storage_gb(req.storage_limit_gb)?;
        if req.expose_publicly {
            let port = req.optional_public_port.ok_or_else(|| {
                Error::Validation(
                    "optional_public_port required when expose_publicly is true".into(),
                )
            })?;
            validate_public_port(port)?;
        }
        Ok(slug)
    }

    /// Provision volume, network, and container. Does not wait for healthy.
    pub async fn provision(
        &self,
        cluster_id: Uuid,
        slug: &str,
        req: &CreateClusterRequest,
        admin_password: &str,
    ) -> Result<(ResourceNames, ProvisionedResources)> {
        let names = self.resource_names(slug);
        let mut resources = ProvisionedResources::default();
        let image = Config::postgres_image(&req.postgres_version)?.to_string();

        let mut labels = HashMap::new();
        labels.insert("pgpanel.cluster_id".into(), cluster_id.to_string());
        labels.insert("pgpanel.slug".into(), slug.to_string());

        // 1. Ensure image
        self.docker.ensure_image(&image).await?;

        // 2. Volume
        if !self.docker.volume_exists(&names.volume_name).await? {
            self.docker
                .create_volume(&names.volume_name, labels.clone())
                .await?;
        }
        resources.volume_created = true;

        // 3. Network (private/internal)
        if !self.docker.network_exists(&names.network_name).await? {
            self.docker
                .create_network(&names.network_name, labels.clone())
                .await?;
        }
        resources.network_created = true;

        // 4. Container
        if self.docker.container_exists(&names.container_name).await? {
            // Idempotent: inspect existing
            let inspect = self.docker.inspect_container(&names.container_name).await?;
            resources.container_created = true;
            resources.container_id = Some(inspect.id);
            if inspect.running {
                resources.container_started = true;
            }
            return Ok((names, resources));
        }

        let public_port = if req.expose_publicly {
            req.optional_public_port
        } else {
            None
        };

        let spec = PostgresContainerSpec {
            container_name: names.container_name.clone(),
            volume_name: names.volume_name.clone(),
            network_name: names.network_name.clone(),
            image,
            postgres_password: admin_password.to_string(),
            cpu_limit: req.cpu_limit,
            memory_mb: req.memory_mb,
            public_port,
            labels: labels.into_iter().collect(),
        };

        let container_id = self.docker.create_postgres_container(&spec).await?;
        resources.container_created = true;
        resources.container_id = Some(container_id.clone());

        // 5. Start
        self.docker.start_container(&container_id).await?;
        resources.container_started = true;

        Ok((names, resources))
    }

    pub async fn wait_healthy(&self, container: &str) -> Result<()> {
        self.docker
            .wait_healthy(container, Duration::from_secs(120))
            .await
    }

    pub async fn start(&self, container: &str) -> Result<()> {
        self.docker.start_container(container).await?;
        self.wait_healthy(container).await
    }

    pub async fn stop(&self, container: &str) -> Result<()> {
        self.docker.stop_container(container, 30).await
    }

    pub async fn restart(&self, container: &str) -> Result<()> {
        // stop then start for clean restart
        if let Err(e) = self.docker.stop_container(container, 30).await {
            info!(error = %e, "stop during restart (may already be stopped)");
        }
        self.docker.start_container(container).await?;
        self.wait_healthy(container).await
    }

    /// Delete Docker resources according to mode.
    /// Never auto-deletes volume unless `delete_volume` is true.
    pub async fn deprovision(
        &self,
        container_name: &str,
        network_name: &str,
        volume_name: &str,
        delete_volume: bool,
    ) -> Result<()> {
        if self.docker.container_exists(container_name).await? {
            if let Err(e) = self.docker.stop_container(container_name, 15).await {
                error!(error = %e, "stop before remove");
            }
            self.docker.remove_container(container_name, true).await?;
        }

        if self.docker.network_exists(network_name).await? {
            if let Err(e) = self.docker.remove_network(network_name).await {
                error!(error = %e, "remove network");
            }
        }

        if delete_volume {
            if self.docker.volume_exists(volume_name).await? {
                self.docker.remove_volume(volume_name, true).await?;
            }
        } else {
            info!(%volume_name, "keeping volume (not deleted)");
        }

        Ok(())
    }
}
