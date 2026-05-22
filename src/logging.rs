use std::io;
/// Structured logging and observability utilities for MemoBuild
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Registry,
};

/// Initialize structured logging with optional JSON output
pub fn init_logging(json_output: bool) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("memobuild=info"));

    let registry = Registry::default().with(env_filter);

    if json_output {
        // JSON output for structured logging aggregation
        registry
            .with(
                fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_thread_ids(true)
                    .with_span_events(FmtSpan::ACTIVE),
            )
            .init();
    } else {
        // Pretty console output
        registry
            .with(
                fmt::layer()
                    .with_writer(io::stderr)
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_span_events(FmtSpan::CLOSE),
            )
            .init();
    }

    Ok(())
}

/// Metrics collection for build operations
#[derive(Debug, Clone, Default)]
pub struct BuildMetrics {
    pub total_builds: u64,
    pub successful_builds: u64,
    pub failed_builds: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_duration_ms: u64,
    pub total_artifacts_bytes: u64,
}

impl BuildMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f64) / (total as f64)
        }
    }

    pub fn average_build_time_ms(&self) -> f64 {
        if self.total_builds == 0 {
            0.0
        } else {
            (self.total_duration_ms as f64) / (self.total_builds as f64)
        }
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.successful_builds + self.failed_builds;
        if total == 0 {
            0.0
        } else {
            (self.successful_builds as f64) / (total as f64)
        }
    }
}

/// Span event logging for cache operations
#[macro_export]
macro_rules! log_cache_hit {
    ($hash:expr, $size:expr) => {
        tracing::debug!(
            hash = %$hash[..8.min($hash.len())],
            size_bytes = $size,
            "Cache hit"
        );
    };
}

#[macro_export]
macro_rules! log_cache_miss {
    ($hash:expr) => {
        tracing::debug!(hash = %$hash[..8.min($hash.len())], "Cache miss");
    };
}

#[macro_export]
macro_rules! log_cache_store {
    ($hash:expr, $size:expr) => {
        tracing::debug!(
            hash = %$hash[..8.min($hash.len())],
            size_bytes = $size,
            "Storing in cache"
        );
    };
}

#[macro_export]
macro_rules! log_build_start {
    ($dockerfile:expr) => {
        tracing::info!(dockerfile = $dockerfile, "Build started");
    };
}

#[macro_export]
macro_rules! log_build_complete {
    ($duration_ms:expr, $dirty_nodes:expr, $cached_nodes:expr) => {
        tracing::info!(
            duration_ms = $duration_ms,
            dirty_nodes = $dirty_nodes,
            cached_nodes = $cached_nodes,
            "Build completed"
        );
    };
}

#[macro_export]
macro_rules! log_cas_verify_fail {
    ($expected:expr, $actual:expr, $size:expr) => {
        tracing::error!(
            expected = %$expected[..8.min($expected.len())],
            actual = %$actual[..8.min($actual.len())],
            size = $size,
            "CAS verification failed"
        );
    };
}

#[macro_export]
macro_rules! log_remote_operation {
    ($operation:expr, $status:expr, $duration_ms:expr) => {
        tracing::debug!(
            operation = $operation,
            status = $status,
            duration_ms = $duration_ms,
            "Remote operation completed"
        );
    };
}

/// Event types for distributed tracing
#[derive(Debug, Clone)]
pub enum TraceEvent {
    BuildStarted { dockerfile: String },
    NodeExecuting { node_id: usize, node_name: String },
    NodeCached { node_id: usize },
    CacheHit { hash: String, duration_ms: u64 },
    CacheMiss { hash: String },
    RemoteSync { direction: String, bytes: u64 },
    LayerBuilt { hash: String, size_bytes: u64 },
    Error { component: String, message: String },
}

impl std::fmt::Display for TraceEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuildStarted { dockerfile } => {
                write!(f, "Build started: {}", dockerfile)
            }
            Self::NodeExecuting { node_id, node_name } => {
                write!(f, "Executing node {} ({})", node_id, node_name)
            }
            Self::NodeCached { node_id } => {
                write!(f, "Using cached node {}", node_id)
            }
            Self::CacheHit { hash, duration_ms } => {
                write!(
                    f,
                    "Cache hit for {} ({}ms)",
                    &hash[..8.min(hash.len())],
                    duration_ms
                )
            }
            Self::CacheMiss { hash } => {
                write!(f, "Cache miss for {}", &hash[..8.min(hash.len())])
            }
            Self::RemoteSync { direction, bytes } => {
                write!(f, "Remote sync {} bytes {}", bytes, direction)
            }
            Self::LayerBuilt { hash, size_bytes } => {
                write!(
                    f,
                    "Layer built {} ({} bytes)",
                    &hash[..8.min(hash.len())],
                    size_bytes
                )
            }
            Self::Error { component, message } => {
                write!(f, "Error in {}: {}", component, message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_metrics_calculation() {
        let mut metrics = BuildMetrics::new();
        metrics.successful_builds = 90;
        metrics.failed_builds = 10;
        metrics.total_builds = 100;

        assert!((metrics.success_rate() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_cache_hit_rate_calculation() {
        let mut metrics = BuildMetrics::new();
        metrics.cache_hits = 75;
        metrics.cache_misses = 25;

        assert!((metrics.cache_hit_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_average_build_time() {
        let mut metrics = BuildMetrics::new();
        metrics.total_builds = 10;
        metrics.total_duration_ms = 5000;

        assert!((metrics.average_build_time_ms() - 500.0).abs() < 0.1);
    }

    #[test]
    fn test_zero_metrics() {
        let metrics = BuildMetrics::new();
        assert_eq!(metrics.cache_hit_rate(), 0.0);
        assert_eq!(metrics.success_rate(), 0.0);
        assert_eq!(metrics.average_build_time_ms(), 0.0);
    }

    #[test]
    fn test_trace_event_display() {
        let event = TraceEvent::BuildStarted {
            dockerfile: "Dockerfile".to_string(),
        };
        let display = event.to_string();
        assert!(display.contains("Build started"));
    }

    #[test]
    fn test_cache_hit_event_formatting() {
        let event = TraceEvent::CacheHit {
            hash: "abc123def456".to_string(),
            duration_ms: 42,
        };
        let display = event.to_string();
        assert!(display.contains("Cache hit"));
        assert!(display.contains("abc123de"));
        assert!(display.contains("42ms"));
    }
}
