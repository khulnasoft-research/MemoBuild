//! Redis L1 Distributed Cache
//!
//! Provides a Redis-backed `RemoteCache` implementation for fast cache lookups
//! before falling back to object storage. Configured via `MEMOBUILD_REDIS_URL`.

use crate::dashboard::BuildEvent;
use crate::graph::BuildGraph;
use crate::remote_cache::RemoteCache;
use anyhow::Result;
use async_trait::async_trait;
use fred::prelude::*;
use fred::types::Scanner;

/// Redis-backed L1 distributed cache.
///
/// Keys follow the pattern:
///   `memobuild:cache:{hash}`       — artifact blobs
///   `memobuild:layer:{hash}`       — OCI layers
///   `memobuild:node:{hash}:layers` — layer manifest (JSON)
pub struct RedisCache {
    client: RedisClient,
    ttl_secs: u64,
    key_prefix: String,
}

impl RedisCache {
    pub async fn new(url: &str, ttl_secs: u64, key_prefix: Option<String>) -> Result<Self> {
        let config = RedisConfig::from_url(url)?;
        let client = RedisClient::new(config, None, None);

        let _ = client.connect();
        client.wait_for_connect().await?;

        tracing::info!("Redis cache connected to {}", url);

        Ok(Self {
            client,
            ttl_secs,
            key_prefix: key_prefix.unwrap_or_else(|| "memobuild".to_string()),
        })
    }

    fn blob_key(&self, hash: &str) -> String {
        format!("{}:cache:{}", self.key_prefix, hash)
    }

    fn layer_key(&self, hash: &str) -> String {
        format!("{}:layer:{}", self.key_prefix, hash)
    }

    fn node_layers_key(&self, hash: &str) -> String {
        format!("{}:node:{}:layers", self.key_prefix, hash)
    }

    async fn publish_evict(&self, hash: &str) {
        let channel = format!("{}:evict", self.key_prefix);
        if let Err(e) = self.client.publish::<(), _, _>(&channel, hash).await {
            tracing::warn!("Failed to publish eviction for {}: {}", hash, e);
        }
    }
}

#[async_trait]
impl RemoteCache for RedisCache {
    async fn has(&self, hash: &str) -> Result<bool> {
        let exists: bool = self.client.exists(&self.blob_key(hash)).await?;
        Ok(exists)
    }

    async fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let data: Option<fred::types::RedisValue> =
            self.client.get(&self.blob_key(hash)).await?;
        match data {
            Some(val) => Ok(Some(val.convert::<Vec<u8>>()?)),
            None => Ok(None),
        }
    }

    async fn put(&self, hash: &str, data: &[u8]) -> Result<()> {
        let key = self.blob_key(hash);
        if self.ttl_secs > 0 {
            let _: () = self
                .client
                .set(&key, data, Some(Expiration::EX(self.ttl_secs as i64)), None, false)
                .await?;
        } else {
            let _: () = self.client.set(&key, data, None, None, false).await?;
        }
        Ok(())
    }

    async fn has_layer(&self, hash: &str) -> Result<bool> {
        let exists: bool = self.client.exists(&self.layer_key(hash)).await?;
        Ok(exists)
    }

    async fn get_layer(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let data: Option<fred::types::RedisValue> =
            self.client.get(&self.layer_key(hash)).await?;
        match data {
            Some(val) => Ok(Some(val.convert::<Vec<u8>>()?)),
            None => Ok(None),
        }
    }

    async fn put_layer(&self, hash: &str, data: &[u8]) -> Result<()> {
        let key = self.layer_key(hash);
        if self.ttl_secs > 0 {
            let _: () = self
                .client
                .set(&key, data, Some(Expiration::EX(self.ttl_secs as i64)), None, false)
                .await?;
        } else {
            let _: () = self.client.set(&key, data, None, None, false).await?;
        }
        Ok(())
    }

    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        let val: Option<String> = self.client.get(&self.node_layers_key(hash)).await?;
        match val {
            Some(json_str) => {
                let layers: Vec<String> = serde_json::from_str(&json_str)?;
                Ok(Some(layers))
            }
            None => Ok(None),
        }
    }

    async fn register_node_layers(
        &self,
        hash: &str,
        layers: &[String],
        _total_size: u64,
    ) -> Result<()> {
        let key = self.node_layers_key(hash);
        let json = serde_json::to_string(layers)?;
        if self.ttl_secs > 0 {
            let _: () = self
                .client
                .set(&key, json.as_str(), Some(Expiration::EX(self.ttl_secs as i64)), None, false)
                .await?;
        } else {
            let _: () = self.client.set(&key, json.as_str(), None, None, false).await?;
        }
        Ok(())
    }

    async fn report_build_event(&self, _event: BuildEvent) -> Result<()> {
        Ok(())
    }

    async fn report_dag(&self, _dag: &BuildGraph) -> Result<()> {
        Ok(())
    }

    async fn report_analytics(&self, _dirty: u32, _cached: u32, _duration_ms: u64) -> Result<()> {
        Ok(())
    }
}

impl RedisCache {
    pub async fn evict(&self, hash: &str) -> Result<()> {
        let _: () = self.client.del(&self.blob_key(hash)).await?;
        let _: () = self.client.del(&self.layer_key(hash)).await?;
        let _: () = self.client.del(&self.node_layers_key(hash)).await?;
        self.publish_evict(hash).await;
        Ok(())
    }

    pub async fn flush(&self) -> Result<()> {
        let pattern = format!("{}:*", self.key_prefix);
        let mut scan_stream = self.client.scan(&pattern, None, None);
        let mut all_keys: Vec<String> = Vec::new();
        while let Some(result) = futures::StreamExt::next(&mut scan_stream).await {
            let mut page: fred::types::ScanResult = result?;
            let keys: Vec<String> = page
                .take_results()
                .unwrap_or_default()
                .into_iter()
                .map(|k| String::from_utf8_lossy(&k.into_bytes()).to_string())
                .collect();
            all_keys.extend(keys);
        }
        if !all_keys.is_empty() {
            let _: () = self.client.del(all_keys).await?;
        }
        tracing::info!("Flushed Redis cache namespace: {}", self.key_prefix);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_redis_cache_roundtrip() {
        let cache = RedisCache::new("redis://localhost:6379", 300, Some("test".into()))
            .await
            .unwrap();

        let hash = "deadbeef12345678";
        let data = b"hello-redis";

        cache.put(hash, data).await.unwrap();
        assert!(cache.has(hash).await.unwrap());

        let got = cache.get(hash).await.unwrap().unwrap();
        assert_eq!(got, data);

        cache.evict(hash).await.unwrap();
        assert!(!cache.has(hash).await.unwrap());
    }
}
