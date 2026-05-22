use std::collections::HashMap;
/// Load testing utilities for MemoBuild scalability analysis
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Performance metrics collected during load test
#[derive(Debug, Clone)]
pub struct LoadTestMetrics {
    pub total_operations: u64,
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub total_duration_ms: u64,
    pub min_latency_ms: u64,
    pub max_latency_ms: u64,
    pub avg_latency_ms: u64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub throughput_ops_per_sec: f64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub errors_by_type: HashMap<String, u64>,
}

impl LoadTestMetrics {
    pub fn new() -> Self {
        Self {
            total_operations: 0,
            successful_operations: 0,
            failed_operations: 0,
            total_duration_ms: 0,
            min_latency_ms: u64::MAX,
            max_latency_ms: 0,
            avg_latency_ms: 0,
            p50_latency_ms: 0,
            p95_latency_ms: 0,
            p99_latency_ms: 0,
            throughput_ops_per_sec: 0.0,
            cache_hits: 0,
            cache_misses: 0,
            errors_by_type: HashMap::new(),
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.cache_hits as f64 / total as f64) * 100.0
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            0.0
        } else {
            (self.successful_operations as f64 / self.total_operations as f64) * 100.0
        }
    }

    pub fn error_rate(&self) -> f64 {
        100.0 - self.success_rate()
    }
}

impl Default for LoadTestMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for load test scenarios
#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    pub num_clients: usize,
    pub duration_secs: u64,
    pub requests_per_client: usize,
    pub cache_hit_ratio_target: f64,
    pub timeout_secs: u64,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            num_clients: 10,
            duration_secs: 60,
            requests_per_client: 100,
            cache_hit_ratio_target: 0.75,
            timeout_secs: 30,
        }
    }
}

/// Shared state for coordinating load test execution
#[derive(Debug, Clone)]
pub struct LoadTestState {
    pub total_ops: Arc<AtomicU64>,
    pub successful_ops: Arc<AtomicU64>,
    pub failed_ops: Arc<AtomicU64>,
    pub cache_hits: Arc<AtomicU64>,
    pub cache_misses: Arc<AtomicU64>,
    pub latencies: Arc<parking_lot::Mutex<Vec<u64>>>,
}

impl LoadTestState {
    pub fn new() -> Self {
        Self {
            total_ops: Arc::new(AtomicU64::new(0)),
            successful_ops: Arc::new(AtomicU64::new(0)),
            failed_ops: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
            latencies: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }
}

impl Default for LoadTestState {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadTestState {
    pub fn record_operation(&self, latency_ms: u64, success: bool, cache_hit: bool) {
        self.total_ops.fetch_add(1, Ordering::Relaxed);

        if success {
            self.successful_ops.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_ops.fetch_add(1, Ordering::Relaxed);
        }

        if cache_hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        self.latencies.lock().push(latency_ms);
    }

    pub fn get_metrics(&self, duration_ms: u64) -> LoadTestMetrics {
        let total = self.total_ops.load(Ordering::Relaxed);
        let successful = self.successful_ops.load(Ordering::Relaxed);
        let failed = self.failed_ops.load(Ordering::Relaxed);
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);

        let mut latencies = self.latencies.lock().clone();
        latencies.sort_unstable();

        let (min_lat, max_lat, avg_lat) = if !latencies.is_empty() {
            let min = latencies[0];
            let max = latencies[latencies.len() - 1];
            let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;
            (min, max, avg)
        } else {
            (0, 0, 0)
        };

        let p50 = latencies.get(latencies.len() / 2).copied().unwrap_or(0);
        let p95 = latencies
            .get((latencies.len() * 95) / 100)
            .copied()
            .unwrap_or(0);
        let p99 = latencies
            .get((latencies.len() * 99) / 100)
            .copied()
            .unwrap_or(0);

        let throughput = if duration_ms > 0 {
            total as f64 / (duration_ms as f64 / 1000.0)
        } else {
            0.0
        };

        LoadTestMetrics {
            total_operations: total,
            successful_operations: successful,
            failed_operations: failed,
            total_duration_ms: duration_ms,
            min_latency_ms: min_lat,
            max_latency_ms: max_lat,
            avg_latency_ms: avg_lat,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            throughput_ops_per_sec: throughput,
            cache_hits: hits,
            cache_misses: misses,
            errors_by_type: HashMap::new(),
        }
    }
}

/// Trait for implementing custom load test scenarios
#[async_trait::async_trait]
pub trait LoadTestScenario: Send + Sync {
    /// Run a single operation for this scenario
    async fn execute(&self) -> (bool, bool); // (success, cache_hit)

    /// Name of this scenario
    fn name(&self) -> &'static str;
}

/// Build a simple cache operation load test
pub struct CacheLoadTest {
    pub cache_size: usize,
    pub hit_probability: f64,
}

#[async_trait::async_trait]
impl LoadTestScenario for CacheLoadTest {
    async fn execute(&self) -> (bool, bool) {
        // Simulate cache operation with roughly hit_probability hit rate
        let hit = rand::random::<f64>() < self.hit_probability;
        (true, hit) // Always succeeds for basic test
    }

    fn name(&self) -> &'static str {
        "CacheLoadTest"
    }
}

/// Run a load test scenario concurrently
pub async fn run_load_test(
    scenario: Arc<dyn LoadTestScenario>,
    config: LoadTestConfig,
) -> LoadTestMetrics {
    let state = LoadTestState::new();
    let test_start = Instant::now();
    let deadline = test_start + Duration::from_secs(config.duration_secs);

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    for _ in 0..config.num_clients {
        let scenario = Arc::clone(&scenario);
        let state = state.clone();
        let requests = config.requests_per_client;

        let handle = tokio::spawn(async move {
            for _ in 0..requests {
                if Instant::now() > deadline {
                    break;
                }

                let op_start = Instant::now();
                let (success, cache_hit) = scenario.execute().await;
                let latency_ms = op_start.elapsed().as_millis() as u64;

                state.record_operation(latency_ms, success, cache_hit);

                // Small delay between requests
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete or timeout
    for handle in handles {
        let _ = tokio::time::timeout(
            Duration::from_secs(config.timeout_secs + config.duration_secs),
            handle,
        )
        .await;
    }

    let total_ms = test_start.elapsed().as_millis() as u64;
    state.get_metrics(total_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_calculations() {
        let state = LoadTestState::new();

        // Record some operations
        state.record_operation(100, true, true);
        state.record_operation(150, true, false);
        state.record_operation(200, false, false);

        let metrics = state.get_metrics(1000);

        assert_eq!(metrics.total_operations, 3);
        assert_eq!(metrics.successful_operations, 2);
        assert_eq!(metrics.failed_operations, 1);
        assert_eq!(metrics.cache_hits, 1);
        assert_eq!(metrics.cache_misses, 2);
    }

    #[test]
    fn test_cache_hit_rate() {
        let mut metrics = LoadTestMetrics::new();
        metrics.cache_hits = 75;
        metrics.cache_misses = 25;

        assert!((metrics.cache_hit_rate() - 75.0).abs() < 0.1);
    }

    #[test]
    fn test_success_rate() {
        let mut metrics = LoadTestMetrics::new();
        metrics.total_operations = 100;
        metrics.successful_operations = 95;
        metrics.failed_operations = 5;

        assert!((metrics.success_rate() - 95.0).abs() < 0.1);
        assert!((metrics.error_rate() - 5.0).abs() < 0.1);
    }
}
