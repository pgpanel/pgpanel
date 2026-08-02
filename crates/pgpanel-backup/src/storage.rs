use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::info;
use url::Url;

use pgpanel_core::error::{Error, Result};

type HmacSha256 = Hmac<sha2::Sha256>;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn test(&self) -> Result<()> {
        Ok(())
    }
    fn kind(&self) -> &'static str;
}

pub struct LocalStorage {
    pub root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let safe = key.replace("..", "_").replace('\\', "/");
        self.root.join(safe)
    }
}

#[async_trait]
impl StorageBackend for LocalStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Internal(format!("mkdir: {e}")))?;
        }
        let mut f = fs::File::create(&path)
            .await
            .map_err(|e| Error::Internal(format!("create {path:?}: {e}")))?;
        f.write_all(data)
            .await
            .map_err(|e| Error::Internal(format!("write: {e}")))?;
        f.flush()
            .await
            .map_err(|e| Error::Internal(format!("flush: {e}")))?;
        info!(%key, bytes = data.len(), "local backup stored");
        Ok(key.to_string())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.path_for(key);
        fs::read(&path)
            .await
            .map_err(|e| Error::NotFound(format!("backup {key}: {e}")))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_for(key);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| Error::Internal(format!("delete: {e}")))?;
        }
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.path_for(key).exists())
    }

    async fn test(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .await
            .map_err(|e| Error::Internal(format!("storage directory: {e}")))
    }

    fn kind(&self) -> &'static str {
        "local"
    }
}

#[derive(Clone, Debug)]
pub struct S3StorageConfig {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub prefix: String,
    pub path_style: bool,
    pub tls_verify: bool,
}

pub struct S3Storage {
    local: LocalStorage,
    config: S3StorageConfig,
    client: reqwest::Client,
}

impl S3Storage {
    pub fn from_env(local_root: PathBuf) -> Result<Self> {
        Self::from_config(
            local_root,
            S3StorageConfig {
                endpoint: std::env::var("S3_ENDPOINT").ok(),
                region: std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
                bucket: std::env::var("S3_BUCKET").unwrap_or_default(),
                access_key: std::env::var("S3_ACCESS_KEY").unwrap_or_default(),
                secret_key: std::env::var("S3_SECRET_KEY").unwrap_or_default(),
                prefix: std::env::var("S3_PREFIX").unwrap_or_else(|_| "pgpanel/".into()),
                path_style: env_bool("S3_PATH_STYLE", true),
                tls_verify: env_bool("S3_TLS_VERIFY", true),
            },
        )
    }

    pub fn from_config(local_root: PathBuf, config: S3StorageConfig) -> Result<Self> {
        if config.bucket.trim().is_empty()
            || config.access_key.trim().is_empty()
            || config.secret_key.trim().is_empty()
        {
            return Err(Error::Validation(
                "S3 bucket, access key and secret key are required".into(),
            ));
        }
        let staging = local_root.join("s3-staging");
        std::fs::create_dir_all(&staging).ok();
        let mut builder = reqwest::Client::builder();
        if !config.tls_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let client = builder
            .build()
            .map_err(|e| Error::Internal(format!("S3 HTTP client: {e}")))?;
        Ok(Self {
            local: LocalStorage::new(staging),
            config,
            client,
        })
    }

    fn meta_key(&self, key: &str) -> String {
        format!(
            "s3://{}/{}{}",
            self.config.bucket,
            self.config.prefix,
            key.trim_start_matches('/')
        )
    }

    fn raw_key<'a>(&self, key: &'a str) -> &'a str {
        let prefix = format!("s3://{}/{}", self.config.bucket, self.config.prefix);
        if let Some(raw) = key.strip_prefix(&prefix) {
            raw.trim_start_matches('/')
        } else {
            key.trim_start_matches('/')
        }
    }

    fn endpoint(&self) -> Result<Url> {
        let endpoint = self.config.endpoint.clone().unwrap_or_else(|| {
            if self.config.region == "auto" {
                "https://s3.amazonaws.com".into()
            } else {
                format!("https://s3.{}.amazonaws.com", self.config.region)
            }
        });
        Url::parse(endpoint.trim_end_matches('/'))
            .map_err(|e| Error::Validation(format!("invalid S3 endpoint: {e}")))
    }

    fn signing_region(&self) -> &str {
        if self.config.region == "auto" && self.config.endpoint.is_none() {
            "us-east-1"
        } else {
            &self.config.region
        }
    }

    fn url_for(&self, key: Option<&str>) -> Result<Url> {
        let mut endpoint = self.endpoint()?;
        let object = key.map(|k| {
            format!("{}{}", self.config.prefix, self.raw_key(k))
        });

        if self.config.path_style {
            let suffix = match object {
                Some(object) => format!("/{}/{}", self.config.bucket, object),
                None => format!("/{}", self.config.bucket),
            };
            endpoint.set_path(&format!(
                "{}{}",
                endpoint.path().trim_end_matches('/'),
                suffix
            ));
        } else {
            let host = endpoint
                .host_str()
                .ok_or_else(|| Error::Validation("S3 endpoint must include a host".into()))?
                .to_string();
            endpoint
                .set_host(Some(&format!("{}.{}", self.config.bucket, host)))
                .map_err(|_| Error::Validation("invalid virtual-host S3 endpoint".into()))?;
            if let Some(object) = object {
                endpoint.set_path(&format!("/{object}"));
            }
        }
        Ok(endpoint)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response> {
        let payload_hash = body
            .as_deref()
            .map(sha256_hex)
            .unwrap_or_else(|| sha256_hex(b""));
        let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let short_date = &amz_date[..8];
        let host_name = url
            .host_str()
            .ok_or_else(|| Error::Validation("S3 endpoint must include a host".into()))?;
        let host = match url.port() {
            Some(port) => format!("{host_name}:{port}"),
            None => host_name.to_string(),
        };
        let canonical_uri = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };
        let canonical_query = url.query().unwrap_or("");
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str(),
            canonical_uri,
            canonical_query,
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let region = self.signing_region();
        let scope = format!("{short_date}/{region}/s3/aws4_request");
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let k_date = hmac_sha256(
            format!("AWS4{}", self.config.secret_key).as_bytes(),
            short_date,
        );
        let k_region = hmac_sha256(&k_date, region);
        let k_service = hmac_sha256(&k_region, "s3");
        let signing_key = hmac_sha256(&k_service, "aws4_request");
        let signature = hex::encode(hmac_sha256(&signing_key, &string_to_sign));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.config.access_key
        );

        let mut request = self
            .client
            .request(method, url)
            .header("host", host)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", amz_date)
            .header("authorization", authorization);
        if let Some(body) = body {
            request = request.body(body);
        }
        request
            .send()
            .await
            .map_err(|e| Error::Backup(format!("S3 request: {e}")))
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<String> {
        self.local.put(key, data).await?;
        let response = self
            .request(
                reqwest::Method::PUT,
                self.url_for(Some(key))?,
                Some(data.to_vec()),
            )
            .await?;
        if !response.status().is_success() {
            return Err(Error::Backup(format!(
                "S3 upload failed: {}",
                response.status()
            )));
        }
        info!(key = %self.meta_key(key), bytes = data.len(), "backup uploaded to S3");
        Ok(self.meta_key(key))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        if let Some(staged) = key.strip_prefix("local-staging:") {
            return self.local.get(staged).await;
        }
        let response = self
            .request(reqwest::Method::GET, self.url_for(Some(key))?, None)
            .await?;
        if !response.status().is_success() {
            return Err(Error::NotFound(format!(
                "S3 object {key}: {}",
                response.status()
            )));
        }
        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| Error::Backup(format!("S3 download: {e}")))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        if let Some(staged) = key.strip_prefix("local-staging:") {
            return self.local.delete(staged).await;
        }
        let response = self
            .request(reqwest::Method::DELETE, self.url_for(Some(key))?, None)
            .await?;
        if !response.status().is_success() && response.status().as_u16() != 404 {
            return Err(Error::Backup(format!(
                "S3 delete failed: {}",
                response.status()
            )));
        }
        self.local.delete(key).await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        if let Some(staged) = key.strip_prefix("local-staging:") {
            return self.local.exists(staged).await;
        }
        let response = self
            .request(reqwest::Method::HEAD, self.url_for(Some(key))?, None)
            .await?;
        Ok(response.status().is_success())
    }

    async fn test(&self) -> Result<()> {
        let response = self
            .request(reqwest::Method::HEAD, self.url_for(None)?, None)
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(Error::Backup(format!(
                "S3 bucket test failed: {}",
                response.status()
            )))
        }
    }

    fn kind(&self) -> &'static str {
        "s3"
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts arbitrary key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

impl S3Storage {
    #[allow(dead_code)]
    fn root(&self) -> &std::path::Path {
        &self.local.root
    }
}
