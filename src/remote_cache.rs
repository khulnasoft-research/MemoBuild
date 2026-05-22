use crate::dashboard::BuildEvent;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCacheEntry {
    pub key: String,
    pub size: u64,
}

#[async_trait]
pub trait RemoteCache: Send + Sync {
    async fn has(&self, hash: &str) -> Result<bool>;
    async fn get(&self, hash: &str) -> Result<Option<Vec<u8>>>;
    async fn put(&self, hash: &str, data: &[u8]) -> Result<()>;

    // Layered cache methods
    async fn has_layer(&self, hash: &str) -> Result<bool>;
    async fn get_layer(&self, hash: &str) -> Result<Option<Vec<u8>>>;
    async fn put_layer(&self, hash: &str, data: &[u8]) -> Result<()>;
    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>>;
    async fn register_node_layers(
        &self,
        hash: &str,
        layers: &[String],
        total_size: u64,
    ) -> Result<()>;

    async fn report_build_event(&self, event: BuildEvent) -> Result<()>;
    async fn report_dag(&self, dag: &crate::graph::BuildGraph) -> Result<()>;
    async fn report_analytics(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()>;
}

#[derive(Clone)]
pub struct HttpRemoteCache {
    base_url: String,
    client: Client,
    #[allow(dead_code)]
    auth_token: Option<String>,
}

impl HttpRemoteCache {
    pub fn new(base_url: String) -> Self {
        Self::with_auth(base_url, None)
    }

    pub fn with_auth(base_url: String, auth_token: Option<String>) -> Self {
        Self::with_tls_and_auth(base_url, None, auth_token)
    }

    pub fn with_tls_config(base_url: String, tls_config: Option<&crate::tls::TlsConfig>) -> Self {
        Self::with_tls_and_auth(base_url, tls_config, None)
    }

    pub fn with_tls_and_auth(
        base_url: String,
        tls_config: Option<&crate::tls::TlsConfig>,
        auth_token: Option<String>,
    ) -> Self {
        let mut builder = Client::builder().default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                "X-MemoBuild-API-Version",
                reqwest::header::HeaderValue::from_static("1.0"),
            );
            if let Some(ref token) = auth_token {
                headers.insert(
                    "Authorization",
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
                );
            }
            headers
        });

        if let Some(tls) = tls_config {
            if let Ok(client_config) = tls.client_config() {
                builder = builder.use_rustls_tls().use_preconfigured_tls(client_config);
            }
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());

        Self { base_url, client, auth_token }
    }
}

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};
use std::time::Duration;

/// Helper for retrying operations with exponential backoff
async fn retry_with_backoff<F, Fut, T>(
    mut operation: F,
    config: &crate::error::RetryConfig,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                if attempt >= config.max_attempts {
                    return Err(anyhow::anyhow!(
                        "Operation failed after {} attempts: {}",
                        config.max_attempts,
                        e
                    ));
                }

                let backoff_ms = crate::error::calculate_backoff(attempt - 1, config);
                eprintln!(
                    "⚠️  Attempt {} failed, retrying in {}ms: {}",
                    attempt, backoff_ms, e
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}

#[async_trait]
impl RemoteCache for HttpRemoteCache {
    async fn has(&self, hash: &str) -> Result<bool> {
        let config = crate::error::RetryConfig::default();
        retry_with_backoff(
            || async {
                let url = format!("{}/cache/{}", self.base_url, hash);
                let resp = self
                    .client
                    .head(&url)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await?;
                Ok(resp.status().is_success())
            },
            &config,
        )
        .await
    }

    async fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let config = crate::error::RetryConfig::default();
        retry_with_backoff(
            || async {
                let url = format!("{}/cache/{}", self.base_url, hash);
                let resp = self
                    .client
                    .get(&url)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await?;

                if resp.status().is_success() {
                    let compressed_data = resp.bytes().await?;

                    // Decompress
                    let mut decoder = GzDecoder::new(&compressed_data[..]);
                    let mut decompressed_data = Vec::new();
                    decoder.read_to_end(&mut decompressed_data)?;

                    Ok(Some(decompressed_data))
                } else if resp.status() == 404 {
                    Ok(None)
                } else {
                    anyhow::bail!("Remote cache error: {}", resp.status());
                }
            },
            &config,
        )
        .await
    }

    async fn put(&self, hash: &str, data: &[u8]) -> Result<()> {
        // Incremental Layer Update: check if exists before uploading
        if self.has(hash).await? {
            println!("   (skip upload: remote already has {})", &hash[..8]);
            return Ok(());
        }

        let config = crate::error::RetryConfig::default();
        retry_with_backoff(
            || async {
                let url = format!("{}/cache/{}", self.base_url, hash);

                // Build artifact compression
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(data)?;
                let compressed_data = encoder.finish()?;

                let resp = self
                    .client
                    .put(&url)
                    .timeout(Duration::from_secs(60))
                    .body(compressed_data)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    anyhow::bail!("Failed to upload to remote cache: {}", resp.status());
                }
                Ok(())
            },
            &config,
        )
        .await
    }

    async fn has_layer(&self, hash: &str) -> Result<bool> {
        let url = format!("{}/cache/layer/{}", self.base_url, hash);
        let resp = self.client.head(&url).send().await?;
        Ok(resp.status().is_success())
    }

    async fn get_layer(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let url = format!("{}/cache/layer/{}", self.base_url, hash);
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let compressed_data = resp.bytes().await?;
            // Decompress
            let mut decoder = GzDecoder::new(&compressed_data[..]);
            let mut decompressed_data = Vec::new();
            decoder.read_to_end(&mut decompressed_data)?;
            Ok(Some(decompressed_data))
        } else if resp.status() == 404 {
            Ok(None)
        } else {
            anyhow::bail!("Remote layer cache error: {}", resp.status());
        }
    }

    async fn put_layer(&self, hash: &str, data: &[u8]) -> Result<()> {
        let url = format!("{}/cache/layer/{}", self.base_url, hash);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        let compressed_data = encoder.finish()?;
        let resp = self.client.put(&url).body(compressed_data).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to upload layer to remote cache: {}", resp.status());
        }
        Ok(())
    }

    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        let url = format!("{}/cache/node/{}/layers", self.base_url, hash);
        let resp = self.client.get(&url).send().await?;
        if resp.status().is_success() {
            let layers: Vec<String> = resp.json().await?;
            Ok(Some(layers))
        } else if resp.status() == 404 {
            Ok(None)
        } else {
            anyhow::bail!("Failed to get node layers: {}", resp.status());
        }
    }

    async fn register_node_layers(
        &self,
        hash: &str,
        layers: &[String],
        total_size: u64,
    ) -> Result<()> {
        let url = format!("{}/cache/node/{}/layers", self.base_url, hash);
        let payload = serde_json::json!({
            "layers": layers,
            "total_size": total_size
        });
        let resp = self.client.post(&url).json(&payload).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to register node layers: {}", resp.status());
        }
        Ok(())
    }

    async fn report_build_event(&self, event: BuildEvent) -> Result<()> {
        let url = format!("{}/build-event", self.base_url);
        let resp = self.client.post(&url).json(&event).send().await?;
        if !resp.status().is_success() {
            eprintln!("Failed to report build event: {}", resp.status());
        }
        Ok(())
    }

    async fn report_dag(&self, dag: &crate::graph::BuildGraph) -> Result<()> {
        let url = format!("{}/dag", self.base_url);
        let resp = self.client.post(&url).json(dag).send().await?;
        if !resp.status().is_success() {
            eprintln!("Failed to report DAG: {}", resp.status());
        }
        Ok(())
    }

    async fn report_analytics(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()> {
        let url = format!("{}/analytics", self.base_url);
        let data = serde_json::json!({
            "dirty": dirty,
            "cached": cached,
            "duration_ms": duration_ms
        });

        let resp = self.client.post(&url).json(&data).send().await?;

        if !resp.status().is_success() {
            eprintln!("Failed to report analytics: {}", resp.status());
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Helper to test retry_with_backoff logic
    pub async fn test_retry_with_backoff<F, Fut, T>(
        operation: F,
        config: &crate::error::RetryConfig,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        retry_with_backoff(operation, config).await
    }
}
