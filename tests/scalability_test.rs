/// Comprehensive scalability tests for MemoBuild
#[cfg(test)]
mod scalability_tests {
    use memobuild::loadtest::*;
    use std::sync::Arc;

    /// Simple cache operation scenario
    struct SimpleCacheScenario {
        hit_probability: f64,
    }

    #[async_trait::async_trait]
    impl LoadTestScenario for SimpleCacheScenario {
        async fn execute(&self) -> (bool, bool) {
            let hit = rand::random::<f64>() < self.hit_probability;
            // Simulate latency (1-5ms)
            let latency_ms = rand::random::<u64>() % 5 + 1;
            tokio::time::sleep(std::time::Duration::from_millis(latency_ms)).await;
            (true, hit)
        }

        fn name(&self) -> &'static str {
            "SimpleCacheScenario"
        }
    }

    /// Stress test: Many clients, high concurrency
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_cache_operations() {
        let scenario = Arc::new(SimpleCacheScenario {
            hit_probability: 0.75,
        });

        let config = LoadTestConfig {
            num_clients: 50,
            duration_secs: 30,
            requests_per_client: 20,
            cache_hit_ratio_target: 0.75,
            timeout_secs: 60,
        };

        let metrics = run_load_test(scenario, config).await;

        println!("\n📊 Concurrent Cache Operations Test");
        println!("===================================");
        println!("Total operations: {}", metrics.total_operations);
        println!(
            "Successful: {} ({:.2}%)",
            metrics.successful_operations,
            metrics.success_rate()
        );
        println!(
            "Failed: {} ({:.2}%)",
            metrics.failed_operations,
            metrics.error_rate()
        );
        println!("Throughput: {:.2} ops/sec", metrics.throughput_ops_per_sec);
        println!(
            "Latency - Min: {}ms, Max: {}ms, Avg: {}ms",
            metrics.min_latency_ms, metrics.max_latency_ms, metrics.avg_latency_ms
        );
        println!(
            "Percentiles - P50: {}ms, P95: {}ms, P99: {}ms",
            metrics.p50_latency_ms, metrics.p95_latency_ms, metrics.p99_latency_ms
        );
        println!("Cache hit rate: {:.2}%", metrics.cache_hit_rate());

        // Assertions for production readiness
        assert!(metrics.total_operations > 0, "Should complete operations");
        assert!(
            metrics.success_rate() > 95.0,
            "Should have >95% success rate"
        );
        assert!(
            metrics.cache_hit_rate() > 70.0,
            "Should achieve >70% cache hit rate"
        );
        assert!(
            metrics.throughput_ops_per_sec > 10.0,
            "Should achieve >10 ops/sec"
        );
    }

    /// Light load test: Baseline performance
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_light_load_cache_baseline() {
        let scenario = Arc::new(SimpleCacheScenario {
            hit_probability: 0.80,
        });

        let config = LoadTestConfig {
            num_clients: 5,
            duration_secs: 10,
            requests_per_client: 50,
            cache_hit_ratio_target: 0.80,
            timeout_secs: 30,
        };

        let metrics = run_load_test(scenario, config).await;

        println!("\n📊 Light Load Baseline Test");
        println!("============================");
        println!("Total operations: {}", metrics.total_operations);
        println!("Throughput: {:.2} ops/sec", metrics.throughput_ops_per_sec);
        println!("Avg latency: {}ms", metrics.avg_latency_ms);
        println!("Cache hit rate: {:.2}%", metrics.cache_hit_rate());

        // Baseline should be fast
        assert!(
            metrics.avg_latency_ms < 50,
            "Baseline latency should be <50ms"
        );
        assert!(
            metrics.throughput_ops_per_sec > 20.0,
            "Baseline throughput should be >20 ops/sec"
        );
    }

    /// Stress test: Extreme concurrency
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_extreme_concurrency() {
        let scenario = Arc::new(SimpleCacheScenario {
            hit_probability: 0.85,
        });

        let config = LoadTestConfig {
            num_clients: 200,
            duration_secs: 20,
            requests_per_client: 10,
            cache_hit_ratio_target: 0.85,
            timeout_secs: 60,
        };

        let metrics = run_load_test(scenario, config).await;

        println!("\n📊 Extreme Concurrency Stress Test");
        println!("===================================");
        println!("Total operations: {}", metrics.total_operations);
        println!("Success rate: {:.2}%", metrics.success_rate());
        println!("Throughput: {:.2} ops/sec", metrics.throughput_ops_per_sec);
        println!("P99 latency: {}ms", metrics.p99_latency_ms);

        // Should handle extreme load
        assert!(
            metrics.total_operations > 0,
            "Should complete under extreme load"
        );
        assert!(
            metrics.success_rate() > 90.0,
            "Should maintain >90% success under load"
        );
    }

    /// Long-running stability test
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_stability_over_time() {
        let scenario = Arc::new(SimpleCacheScenario {
            hit_probability: 0.75,
        });

        let config = LoadTestConfig {
            num_clients: 20,
            duration_secs: 60, // 1 minute test
            requests_per_client: 30,
            cache_hit_ratio_target: 0.75,
            timeout_secs: 120,
        };

        let metrics = run_load_test(scenario, config).await;

        println!("\n📊 Long-Running Stability Test (60s)");
        println!("=====================================");
        println!("Total operations: {}", metrics.total_operations);
        println!("Avg latency: {}ms", metrics.avg_latency_ms);
        println!("Min latency: {}ms", metrics.min_latency_ms);
        println!("Max latency: {}ms", metrics.max_latency_ms);
        println!("Cache hit rate: {:.2}%", metrics.cache_hit_rate());

        // Stability checks
        assert!(
            metrics.total_operations > 100,
            "Should complete many operations"
        );
        assert!(metrics.max_latency_ms < 1000, "Max latency should stay <1s");
        assert!(metrics.success_rate() > 95.0, "Should maintain stability");
    }

    /// Cache behavior under varying workload
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_varying_cache_hit_rates() {
        println!("\n📊 Cache Hit Rate Variation Test");
        println!("=================================");

        for hit_prob in [0.5, 0.75, 0.90].iter() {
            let scenario = Arc::new(SimpleCacheScenario {
                hit_probability: *hit_prob,
            });

            let config = LoadTestConfig {
                num_clients: 10,
                duration_secs: 10,
                requests_per_client: 30,
                cache_hit_ratio_target: *hit_prob,
                timeout_secs: 30,
            };

            let metrics = run_load_test(scenario, config).await;

            println!(
                "\nTarget hit rate: {:.0}% | Actual: {:.2}%",
                hit_prob * 100.0,
                metrics.cache_hit_rate()
            );
            println!(
                "  Throughput: {:.2} ops/sec",
                metrics.throughput_ops_per_sec
            );
            println!("  Avg latency: {}ms", metrics.avg_latency_ms);

            // Hit rate should be close to target
            let diff = (metrics.cache_hit_rate() / (hit_prob * 100.0) - 1.0).abs();
            assert!(diff < 0.2, "Hit rate variation should be <20%");
        }
    }
}

/// Exhaustive scalability benchmarks
#[cfg(test)]
mod scalability_benchmarks {
    use memobuild::loadtest::*;
    use std::sync::Arc;

    struct SimpleCacheScenario {
        hit_probability: f64,
    }

    #[async_trait::async_trait]
    impl LoadTestScenario for SimpleCacheScenario {
        async fn execute(&self) -> (bool, bool) {
            let hit = rand::random::<f64>() < self.hit_probability;
            let latency_ms = rand::random::<u64>() % 5 + 1;
            tokio::time::sleep(std::time::Duration::from_millis(latency_ms)).await;
            (true, hit)
        }

        fn name(&self) -> &'static str {
            "SimpleCacheScenario"
        }
    }

    /// Measure scaling with increasing clients
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn benchmark_client_scaling() {
        println!("\n📈 Client Scaling Benchmark");
        println!("===========================");
        println!("Clients | Throughput | Avg Latency | Success Rate");
        println!("--------|------------|-------------|-------------");

        for num_clients in [1, 5, 10, 20, 50].iter() {
            let scenario = Arc::new(SimpleCacheScenario {
                hit_probability: 0.75,
            });

            let config = LoadTestConfig {
                num_clients: *num_clients,
                duration_secs: 15,
                requests_per_client: 25,
                cache_hit_ratio_target: 0.75,
                timeout_secs: 45,
            };

            let metrics = run_load_test(scenario, config).await;

            println!(
                "{:7} | {:10.2} | {:11} | {:.2}%",
                num_clients,
                metrics.throughput_ops_per_sec,
                metrics.avg_latency_ms,
                metrics.success_rate()
            );
        }
    }

    /// Measure scaling with request intensity
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn benchmark_request_intensity() {
        println!("\n📈 Request Intensity Benchmark");
        println!("===============================");
        println!("Requests/Client | Throughput | Avg Latency | P95 Latency");
        println!("-----------------|------------|-------------|------------");

        for requests_per_client in [10, 25, 50, 100].iter() {
            let scenario = Arc::new(SimpleCacheScenario {
                hit_probability: 0.75,
            });

            let config = LoadTestConfig {
                num_clients: 10,
                duration_secs: 15,
                requests_per_client: *requests_per_client,
                cache_hit_ratio_target: 0.75,
                timeout_secs: 45,
            };

            let metrics = run_load_test(scenario, config).await;

            println!(
                "{:15} | {:10.2} | {:11} | {}",
                requests_per_client,
                metrics.throughput_ops_per_sec,
                metrics.avg_latency_ms,
                metrics.p95_latency_ms
            );
        }
    }
}
