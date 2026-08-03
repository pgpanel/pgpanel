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

    /// Clone provisioner bound to a different Docker host (multi-node).
    pub fn with_docker(&self, docker: DockerClient) -> Self {
        Self {
            docker,
            config: self.config.clone(),
        }
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
            // Idempotent: inspect existing and ensure management network
            let inspect = self.docker.inspect_container(&names.container_name).await?;
            resources.container_created = true;
            resources.container_id = Some(inspect.id);
            if inspect.running {
                resources.container_started = true;
            } else {
                self.docker
                    .start_container(&names.container_name)
                    .await?;
                resources.container_started = true;
            }
            self.attach_management_network(&names.container_name)
                .await?;
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

        // 6. Dual-home onto management network so panel resolves
        //    internal_hostname (pgpanel_pg_<slug>) via Docker DNS.
        self.attach_management_network(&names.container_name)
            .await?;

        Ok((names, resources))
    }

    /// Attach cluster container to the shared management network (idempotent).
    pub async fn attach_management_network(&self, container: &str) -> Result<()> {
        let net = &self.config.management_network;
        if net.is_empty() {
            return Ok(());
        }
        if !self.docker.network_exists(net).await? {
            // Compose should create this; create as internal fallback.
            let mut labels = HashMap::new();
            labels.insert("managed-by".into(), "pgpanel".into());
            labels.insert("pgpanel.role".into(), "management".into());
            self.docker.create_network(net, labels).await?;
        }
        self.docker.connect_network(net, container).await?;
        info!(%container, network = %net, "cluster attached to management network");
        Ok(())
    }

    pub async fn wait_healthy(&self, container: &str) -> Result<()> {
        self.docker
            .wait_healthy(container, Duration::from_secs(120))
            .await
    }

    pub async fn start(&self, container: &str) -> Result<()> {
        self.docker.start_container(container).await?;
        // Ensure dual-home for clusters provisioned before this fix.
        if let Err(e) = self.attach_management_network(container).await {
            error!(error = %e, %container, "management network attach on start");
        }
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
        if let Err(e) = self.attach_management_network(container).await {
            error!(error = %e, %container, "management network attach on restart");
        }
        self.wait_healthy(container).await
    }

    pub async fn recreate_with_public_port(
        &self,
        cluster_id: Uuid,
        slug: &str,
        postgres_version: &str,
        cpu_limit: f64,
        memory_mb: u32,
        admin_password: &str,
        public_port: Option<u16>,
    ) -> Result<String> {
        let names = self.resource_names(slug);
        let image = Config::postgres_image(postgres_version)?.to_string();

        if self.docker.container_exists(&names.container_name).await? {
            if let Err(e) = self.docker.stop_container(&names.container_name, 15).await {
                info!(error = %e, "stop before recreate");
            }
            self.docker
                .remove_container(&names.container_name, true)
                .await?;
        }

        self.docker.ensure_image(&image).await?;

        let mut labels = HashMap::new();
        labels.insert("pgpanel.cluster_id".into(), cluster_id.to_string());
        labels.insert("pgpanel.slug".into(), slug.to_string());

        let spec = PostgresContainerSpec {
            container_name: names.container_name.clone(),
            volume_name: names.volume_name.clone(),
            network_name: names.network_name.clone(),
            image,
            postgres_password: admin_password.to_string(),
            cpu_limit,
            memory_mb,
            public_port,
            labels: labels.into_iter().collect(),
        };

        let container_id = self.docker.create_postgres_container(&spec).await?;
        self.docker.start_container(&container_id).await?;
        self.attach_management_network(&names.container_name)
            .await?;
        self.wait_healthy(&names.container_name).await?;
        Ok(container_id)
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
