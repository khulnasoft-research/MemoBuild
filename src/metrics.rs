//! Prometheus metrics for MemoBuild
//!
//! This module provides global metrics collection for monitoring cache hits,
//! build performance, cluster health, and garbage collection operations.

use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CacheLabels {
    tier: String,
    node_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BuildLabels {
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ClusterLabels {
    region: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ReplicationLabels {
    node_id: String,
}

/// Global metrics registry using prometheus-client
pub struct MetricsRegistry {
    registry: Registry,
    cache_hits: Family<CacheLabels, Counter>,
    cache_misses: Family<CacheLabels, Counter>,
    build_duration: Family<BuildLabels, Histogram>,
    cluster_nodes: Family<ClusterLabels, Gauge>,
    replication_lag: Family<ReplicationLabels, Gauge<f64, AtomicU64>>,
    artifact_size: Histogram,
    gc_deleted: Counter,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let cache_hits = Family::<CacheLabels, Counter>::default();
        registry.register(
            "memobuild_cache_hits_total",
            "Total cache hits",
            cache_hits.clone(),
        );

        let cache_misses = Family::<CacheLabels, Counter>::default();
        registry.register(
            "memobuild_cache_misses_total",
            "Total cache misses",
            cache_misses.clone(),
        );

        let build_duration = Family::<BuildLabels, Histogram>::new_with_constructor(|| {
            Histogram::new(vec![0.1, 0.5, 1.0, 5.0, 30.0, 60.0, 300.0].into_iter())
        });
        registry.register(
            "memobuild_build_duration_seconds",
            "Build duration in seconds",
            build_duration.clone(),
        );

        let cluster_nodes = Family::<ClusterLabels, Gauge>::default();
        registry.register(
            "memobuild_cluster_nodes_total",
            "Number of cluster nodes",
            cluster_nodes.clone(),
        );

        let replication_lag = Family::<ReplicationLabels, Gauge<f64, AtomicU64>>::default();
        registry.register(
            "memobuild_replication_lag_seconds",
            "Replication lag in seconds",
            replication_lag.clone(),
        );

        let artifact_size = Histogram::new(
            vec![1024.0, 10240.0, 102400.0, 1048576.0, 10485760.0, 104857600.0].into_iter(), // 1KB, 10KB, 100KB, 1MB, 10MB, 100MB
        );
        registry.register(
            "memobuild_artifact_size_bytes",
            "Artifact size in bytes",
            artifact_size.clone(),
        );

        let gc_deleted = Counter::default();
        registry.register(
            "memobuild_gc_deleted_total",
            "Total artifacts deleted by GC",
            gc_deleted.clone(),
        );

        Self {
            registry,
            cache_hits,
            cache_misses,
            build_duration,
            cluster_nodes,
            replication_lag,
            artifact_size,
            gc_deleted,
        }
    }

    pub fn inc_cache_hits(&self, tier: &str, node_id: &str) {
        self.cache_hits.get_or_create(&CacheLabels {
            tier: tier.to_string(),
            node_id: node_id.to_string(),
        }).inc();
    }

    pub fn inc_cache_misses(&self, tier: &str, node_id: &str) {
        self.cache_misses.get_or_create(&CacheLabels {
            tier: tier.to_string(),
            node_id: node_id.to_string(),
        }).inc();
    }

    pub fn observe_build_duration(&self, duration_secs: f64, status: &str) {
        self.build_duration.get_or_create(&BuildLabels {
            status: status.to_string(),
        }).observe(duration_secs);
    }

    pub fn set_cluster_nodes(&self, region: &str, status: &str, count: i64) {
        self.cluster_nodes.get_or_create(&ClusterLabels {
            region: region.to_string(),
            status: status.to_string(),
        }).set(count);
    }

    pub fn set_replication_lag(&self, node_id: &str, lag_secs: f64) {
        self.replication_lag.get_or_create(&ReplicationLabels {
            node_id: node_id.to_string(),
        }).set(lag_secs);
    }

    pub fn observe_artifact_size(&self, size_bytes: f64) {
        self.artifact_size.observe(size_bytes);
    }

    pub fn inc_gc_deleted(&self) {
        self.gc_deleted.inc();
    }

    pub fn encode(&self) -> String {
        let mut buffer = String::new();
        encode(&mut buffer, &self.registry).unwrap();
        buffer
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedMetrics = Arc<RwLock<MetricsRegistry>>;

pub fn metrics_registry() -> SharedMetrics {
    Arc::new(RwLock::new(MetricsRegistry::new()))
}
