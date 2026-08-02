//! Multi-node Docker host registry.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use pgpanel_core::error::{Error, Result};

use crate::client::DockerClient;

pub const LOCAL_NODE_ID: &str = "00000000-0000-4000-8000-000000000001";

/// Resolves a Docker client per managed node.
#[derive(Clone)]
pub struct NodeRegistry {
    local: DockerClient,
    remote: Arc<RwLock<HashMap<Uuid, DockerClient>>>,
}

impl NodeRegistry {
    pub fn new(local: DockerClient) -> Self {
        Self {
            local,
            remote: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn local(&self) -> &DockerClient {
        &self.local
    }

    pub fn local_node_id() -> Uuid {
        Uuid::parse_str(LOCAL_NODE_ID).expect("local node uuid")
    }

    /// Connect or reuse a Docker client for the given host URL.
    pub async fn client_for_host(&self, node_id: Uuid, docker_host: Option<&str>) -> Result<DockerClient> {
        if docker_host.is_none() || docker_host == Some("") {
            return Ok(self.local.clone());
        }
        {
            let map = self.remote.read().await;
            if let Some(c) = map.get(&node_id) {
                return Ok(c.clone());
            }
        }
        let host = docker_host.unwrap();
        // Only allow unix:// or tcp:// — never arbitrary shell.
        if !(host.starts_with("unix://") || host.starts_with("tcp://") || host.starts_with("http://") || host.starts_with("https://"))
        {
            return Err(Error::Validation(
                "docker_host must start with unix://, tcp://, http:// or https://".into(),
            ));
        }
        let client = DockerClient::connect(Some(host))?;
        client.ping().await?;
        self.remote.write().await.insert(node_id, client.clone());
        Ok(client)
    }

    pub async fn invalidate(&self, node_id: Uuid) {
        self.remote.write().await.remove(&node_id);
    }

    pub async fn ping_host(&self, docker_host: Option<&str>) -> Result<()> {
        let client = if let Some(h) = docker_host.filter(|s| !s.is_empty()) {
            DockerClient::connect(Some(h))?
        } else {
            self.local.clone()
        };
        client.ping().await
    }
}
