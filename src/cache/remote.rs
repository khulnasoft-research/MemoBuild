use crate::dashboard::BuildEvent;
use crate::graph::BuildGraph;
use anyhow::Result;
use async_trait::async_trait;
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
    async fn report_dag(&self, dag: &BuildGraph) -> Result<()>;
    async fn report_analytics(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()>;
}