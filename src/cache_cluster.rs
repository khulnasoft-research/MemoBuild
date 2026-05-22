//! Distributed Cache Clustering for High Availability
//!
//! This module implements multi-master cache replication with consistent hashing
//! for horizontal scaling and fault tolerance.

use crate::remote_cache::RemoteCache;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for a cache cluster node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNode {
    pub id: String,
    pub address: String,
    pub weight: u32,            // For load balancing
    pub region: Option<String>, // For geo-distribution
}

/// Cluster membership and health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node: ClusterNode,
    pub is_healthy: bool,
    pub last_heartbeat: i64,
    pub cache_size: u64,
    pub connections: u32,
}

/// Distributed cache cluster with multi-master replication
pub struct CacheCluster {
    _local_node: ClusterNode,
    nodes: Arc<RwLock<HashMap<String, NodeStatus>>>,
    ring: Arc<RwLock<ConsistentHashRing>>,
    replication_factor: usize,
}

#[derive(Debug, Clone)]
struct ConsistentHashRing {
    nodes: Vec<(u64, String)>, // (hash, node_id)
    replicas: usize,
}

impl ConsistentHashRing {
    fn new(replicas: usize) -> Self {
        Self {
            nodes: Vec::new(),
            replicas,
        }
    }

    fn add_node(&mut self, node_id: &str) {
        for i in 0..self.replicas {
            let key = format!("{}-{}", node_id, i);
            let hash = self.hash(&key);
            self.nodes.push((hash, node_id.to_string()));
        }
        self.nodes.sort_by_key(|(hash, _)| *hash);
    }

    fn remove_node(&mut self, node_id: &str) {
        self.nodes.retain(|(_, id)| id != node_id);
    }

    fn get_node(&self, key: &str) -> Option<&String> {
        if self.nodes.is_empty() {
            return None;
        }

        let hash = self.hash(key);
        // Find the first node with hash >= key hash
        for (node_hash, node_id) in &self.nodes {
            if *node_hash >= hash {
                return Some(node_id);
            }
        }
        // Wrap around to first node
        Some(&self.nodes[0].1)
    }

    fn hash(&self, key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }
}

impl CacheCluster {
    pub fn new(local_node: ClusterNode, replication_factor: usize) -> Self {
        let mut ring = ConsistentHashRing::new(100); // 100 virtual nodes per physical node
        ring.add_node(&local_node.id);

        Self {
            _local_node: local_node,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            ring: Arc::new(RwLock::new(ring)),
            replication_factor,
        }
    }

    /// Add a node to the cluster
    pub async fn add_node(&self, node: ClusterNode) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        let mut ring = self.ring.write().await;

        let status = NodeStatus {
            node: node.clone(),
            is_healthy: true,
            last_heartbeat: chrono::Utc::now().timestamp(),
            cache_size: 0,
            connections: 0,
        };

        nodes.insert(node.id.clone(), status);
        ring.add_node(&node.id);

        println!("➕ Added cluster node: {}", node.id);
        Ok(())
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&self, node_id: &str) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        let mut ring = self.ring.write().await;

        nodes.remove(node_id);
        ring.remove_node(node_id);

        println!("➖ Removed cluster node: {}", node_id);
        Ok(())
    }

    /// Get the primary node responsible for a key
    pub async fn get_primary_node(&self, key: &str) -> Result<Option<String>> {
        let ring = self.ring.read().await;
        Ok(ring.get_node(key).cloned())
    }

    /// Get replica nodes for a key (for replication)
    pub async fn get_replica_nodes(&self, key: &str) -> Result<Vec<String>> {
        let ring = self.ring.read().await;
        let primary = match ring.get_node(key) {
            Some(node) => node.clone(),
            None => return Ok(Vec::new()),
        };

        let _nodes = self.nodes.read().await;
        let mut replicas = Vec::new();

        // Find next N nodes in the ring for replication
        if let Some(primary_idx) = ring.nodes.iter().position(|(_, id)| id == &primary) {
            for i in 1..=self.replication_factor {
                let replica_idx = (primary_idx + i) % ring.nodes.len();
                let replica_id = &ring.nodes[replica_idx].1;
                if replica_id != &primary && !replicas.contains(replica_id) {
                    replicas.push(replica_id.clone());
                }
            }
        }

        Ok(replicas)
    }

    /// Update node health status
    pub async fn update_node_health(&self, node_id: &str, healthy: bool) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        if let Some(status) = nodes.get_mut(node_id) {
            status.is_healthy = healthy;
            status.last_heartbeat = chrono::Utc::now().timestamp();
        }
        Ok(())
    }

    /// Get local node ID
    pub fn local_node_id(&self) -> String {
        self._local_node.id.clone()
    }

    /// Get cluster status
    pub async fn get_cluster_status(&self) -> Result<ClusterStatus> {
        let nodes = self.nodes.read().await;
        let healthy_nodes = nodes.values().filter(|n| n.is_healthy).count();
        let total_nodes = nodes.len();

        Ok(ClusterStatus {
            total_nodes,
            healthy_nodes,
            replication_factor: self.replication_factor,
            nodes: nodes.values().cloned().collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub replication_factor: usize,
    pub nodes: Vec<NodeStatus>,
}

/// Distributed cache implementation with clustering
pub struct DistributedCache {
    cluster: Arc<CacheCluster>,
    local_cache: Arc<dyn RemoteCache>,
    remote_clients: Arc<RwLock<HashMap<String, Arc<dyn RemoteCache>>>>,
    metrics: crate::metrics::SharedMetrics,
}

impl DistributedCache {
    pub fn new(cluster: Arc<CacheCluster>, local_cache: Arc<dyn RemoteCache>, metrics: crate::metrics::SharedMetrics) -> Self {
        Self {
            cluster,
            local_cache,
            remote_clients: Arc::new(RwLock::new(HashMap::new())),
            metrics,
        }
    }

    /// Add a remote cache client for a cluster node
    pub async fn add_remote_client(&self, node_id: &str, client: Arc<dyn RemoteCache>) {
        let mut clients = self.remote_clients.write().await;
        clients.insert(node_id.to_string(), client);
    }

    /// Get a remote cache client for a node
    async fn get_remote_client(&self, node_id: &str) -> Option<Arc<dyn RemoteCache>> {
        let clients = self.remote_clients.read().await;
        clients.get(node_id).cloned()
    }
}

#[async_trait]
impl RemoteCache for DistributedCache {
    async fn has(&self, hash: &str) -> Result<bool> {
        let _span = crate::cache_span!("has", "distributed");

        // Check local first
        if self.local_cache.has(hash).await? {
            self.metrics.read().await.inc_cache_hits("local", &self.cluster.local_node_id());
            return Ok(true);
        }

        // Check primary node
        if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
            if let Some(client) = self.get_remote_client(&primary_node).await {
                if client.has(hash).await? {
                    self.metrics.read().await.inc_cache_hits("remote", &primary_node);
                    return Ok(true);
                }
            }
        }

        // Check replicas
        let replicas = self.cluster.get_replica_nodes(hash).await?;
        for replica_node in replicas {
            if let Some(client) = self.get_remote_client(&replica_node).await {
                if client.has(hash).await? {
                    self.metrics.read().await.inc_cache_hits("remote", &replica_node);
                    return Ok(true);
                }
            }
        }

        self.metrics.read().await.inc_cache_misses("distributed", &self.cluster.local_node_id());
        Ok(false)
    }

    async fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        // Try local first
        if let Some(data) = self.local_cache.get(hash).await? {
            self.metrics.read().await.inc_cache_hits("local", &self.cluster.local_node_id());
            return Ok(Some(data));
        }

        // Try primary node
        if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
            if let Some(client) = self.get_remote_client(&primary_node).await {
                if let Some(data) = client.get(hash).await? {
                    // Cache locally for future requests
                    self.local_cache.put(hash, &data).await?;
                    self.metrics.read().await.inc_cache_hits("remote", &primary_node);
                    return Ok(Some(data));
                }
            }
        }

        // Try replicas
        let replicas = self.cluster.get_replica_nodes(hash).await?;
        for replica_node in replicas {
            if let Some(client) = self.get_remote_client(&replica_node).await {
                if let Some(data) = client.get(hash).await? {
                    // Cache locally and replicate to primary
                    self.local_cache.put(hash, &data).await?;
                    if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
                        if let Some(primary_client) = self.get_remote_client(&primary_node).await {
                            let _ = primary_client.put(hash, &data).await; // Best effort
                        }
                    }
                    self.metrics.read().await.inc_cache_hits("remote", &replica_node);
                    return Ok(Some(data));
                }
            }
        }

        self.metrics.read().await.inc_cache_misses("distributed", &self.cluster.local_node_id());
        Ok(None)
    }

    async fn put(&self, hash: &str, data: &[u8]) -> Result<()> {
        // Observe artifact size
        self.metrics.read().await.observe_artifact_size(data.len() as f64);

        // Store locally first
        self.local_cache.put(hash, data).await?;

        // Replicate to primary and replicas
        let primary_node = self.cluster.get_primary_node(hash).await?;
        let replica_nodes = self.cluster.get_replica_nodes(hash).await?;

        let mut replication_tasks = Vec::new();

        // Replicate to primary
        if let Some(primary) = primary_node {
            if let Some(client) = self.get_remote_client(&primary).await {
                let hash = hash.to_string();
                let data = data.to_vec();
                let task = tokio::spawn(async move {
                    let _span = crate::replicate_span!(&primary);
                    let _ = client.put(&hash, &data).await;
                });
                replication_tasks.push(task);
            }
        }

        // Replicate to replicas
        for replica in replica_nodes {
            if let Some(client) = self.get_remote_client(&replica).await {
                let hash = hash.to_string();
                let data = data.to_vec();
                let task = tokio::spawn(async move {
                    let _span = crate::replicate_span!(&replica);
                    let _ = client.put(&hash, &data).await;
                });
                replication_tasks.push(task);
            }
        }

        // Wait for replication (with timeout)
        for task in replication_tasks {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(30), task).await;
        }

        Ok(())
    }

    // Layered cache methods delegate to local cache for simplicity
    // In production, these would also be distributed
    async fn has_layer(&self, hash: &str) -> Result<bool> {
        // Check local first
        if self.local_cache.has_layer(hash).await? {
            return Ok(true);
        }

        // Check primary node
        if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
            if let Some(client) = self.get_remote_client(&primary_node).await {
                if client.has_layer(hash).await? {
                    return Ok(true);
                }
            }
        }

        // Check replicas
        let replicas = self.cluster.get_replica_nodes(hash).await?;
        for replica_node in replicas {
            if let Some(client) = self.get_remote_client(&replica_node).await {
                if client.has_layer(hash).await? {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    async fn get_layer(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        // Try local first
        if let Some(data) = self.local_cache.get_layer(hash).await? {
            return Ok(Some(data));
        }

        // Try primary node
        if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
            if let Some(client) = self.get_remote_client(&primary_node).await {
                if let Some(data) = client.get_layer(hash).await? {
                    // Cache locally for future requests
                    self.local_cache.put_layer(hash, &data).await?;
                    return Ok(Some(data));
                }
            }
        }

        // Try replicas
        let replicas = self.cluster.get_replica_nodes(hash).await?;
        for replica_node in replicas {
            if let Some(client) = self.get_remote_client(&replica_node).await {
                if let Some(data) = client.get_layer(hash).await? {
                    // Cache locally and replicate to primary
                    self.local_cache.put_layer(hash, &data).await?;
                    if let Some(primary_node) = self.cluster.get_primary_node(hash).await? {
                        if let Some(primary_client) = self.get_remote_client(&primary_node).await {
                            let _ = primary_client.put_layer(hash, &data).await; // Best effort
                        }
                    }
                    return Ok(Some(data));
                }
            }
        }

        Ok(None)
    }

    async fn put_layer(&self, hash: &str, data: &[u8]) -> Result<()> {
        // Store locally first
        self.local_cache.put_layer(hash, data).await?;

        // Replicate to primary and replicas
        let primary_node = self.cluster.get_primary_node(hash).await?;
        let replica_nodes = self.cluster.get_replica_nodes(hash).await?;

        let mut replication_tasks = Vec::new();

        // Replicate to primary
        if let Some(primary) = primary_node {
            if let Some(client) = self.get_remote_client(&primary).await {
                let hash = hash.to_string();
                let data = data.to_vec();
                let task = tokio::spawn(async move {
                    let _ = client.put_layer(&hash, &data).await;
                });
                replication_tasks.push(task);
            }
        }

        // Replicate to replicas
        for replica in replica_nodes {
            if let Some(client) = self.get_remote_client(&replica).await {
                let hash = hash.to_string();
                let data = data.to_vec();
                let task = tokio::spawn(async move {
                    let _ = client.put_layer(&hash, &data).await;
                });
                replication_tasks.push(task);
            }
        }

        // Wait for replication (with timeout)
        for task in replication_tasks {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(30), task).await;
        }

        Ok(())
    }

    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        self.local_cache.get_node_layers(hash).await
    }

    async fn register_node_layers(
        &self,
        hash: &str,
        layers: &[String],
        total_size: u64,
    ) -> Result<()> {
        self.local_cache
            .register_node_layers(hash, layers, total_size)
            .await
    }

    async fn report_build_event(&self, event: crate::dashboard::BuildEvent) -> Result<()> {
        self.local_cache.report_build_event(event).await
    }

    async fn report_dag(&self, dag: &crate::graph::BuildGraph) -> Result<()> {
        self.local_cache.report_dag(dag).await
    }

    async fn report_analytics(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()> {
        self.local_cache
            .report_analytics(dirty, cached, duration_ms)
            .await
    }
}
