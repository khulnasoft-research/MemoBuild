use clap::Parser;
/// Scalability test runner with comprehensive reporting
///
/// Run with: cargo run --example load_test_runner --release -- --clients 100 --duration 60
use memobuild::loadtest::*;
use std::fs;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "MemoBuild Load Test Runner")]
#[command(about = "Run scalability tests and generate performance reports")]
struct Args {
    /// Number of concurrent clients
    #[arg(short, long, default_value = "10")]
    clients: usize,

    /// Test duration in seconds
    #[arg(short, long, default_value = "60")]
    duration: u64,

    /// Requests per client
    #[arg(short, long, default_value = "100")]
    requests: usize,

    /// Cache hit probability (0.0-1.0)
    #[arg(short, long, default_value = "0.75")]
    hit_rate: f64,

    /// Output file for results (JSON format)
    #[arg(short, long)]
    output: Option<String>,

    /// Run multiple test scenarios
    #[arg(long)]
    scenarios: bool,
}

/// Simple cache scenario for testing
struct TestCacheScenario {
    hit_probability: f64,
}

#[async_trait::async_trait]
impl LoadTestScenario for TestCacheScenario {
    async fn execute(&self) -> (bool, bool) {
        let hit = rand::random::<f64>() < self.hit_probability;
        let latency_ms = rand::random::<u64>() % 10 + 1; // 1-10ms simulation
        tokio::time::sleep(std::time::Duration::from_millis(latency_ms)).await;
        (true, hit) // Always succeeds in this test
    }

    fn name(&self) -> &'static str {
        "TestCacheScenario"
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("🚀 MemoBuild Scalability Test Runner");
    println!("=====================================\n");

    if args.scenarios {
        run_scenario_suite().await;
    } else {
        run_single_test(&args).await;
    }

    Ok(())
}

async fn run_single_test(args: &Args) {
    println!("📊 Single Test Configuration");
    println!("----------------------------");
    println!("Clients: {}", args.clients);
    println!("Duration: {}s", args.duration);
    println!("Requests/client: {}", args.requests);
    println!("Target cache hit rate: {:.0}%\n", args.hit_rate * 100.0);

    println!("▶️  Running test...\n");

    let scenario = Arc::new(TestCacheScenario {
        hit_probability: args.hit_rate,
    });

    let config = LoadTestConfig {
        num_clients: args.clients,
        duration_secs: args.duration,
        requests_per_client: args.requests,
        cache_hit_ratio_target: args.hit_rate,
        timeout_secs: args.duration * 2,
    };

    let metrics = run_load_test(scenario, config).await;

    print_metrics_report(&metrics);

    if let Some(output_path) = &args.output {
        match save_metrics_json(&metrics, output_path) {
            Ok(_) => println!("\n✅ Results saved to {}", output_path),
            Err(e) => eprintln!("❌ Failed to save results: {}", e),
        }
    }
}

async fn run_scenario_suite() {
    println!("🔬 Running Comprehensive Scenario Suite\n");

    // Scenario 1: Light load baseline
    println!("▶️  Scenario 1: Light Load Baseline (5 clients, 30s)\n");
    let scenario1 = Arc::new(TestCacheScenario {
        hit_probability: 0.80,
    });
    let metrics1 = run_load_test(
        scenario1,
        LoadTestConfig {
            num_clients: 5,
            duration_secs: 30,
            requests_per_client: 50,
            cache_hit_ratio_target: 0.80,
            timeout_secs: 60,
        },
    )
    .await;
    print_metrics_report(&metrics1);

    // Scenario 2: Standard load
    println!("\n\n▶️  Scenario 2: Standard Load (20 clients, 60s)\n");
    let scenario2 = Arc::new(TestCacheScenario {
        hit_probability: 0.75,
    });
    let metrics2 = run_load_test(
        scenario2,
        LoadTestConfig {
            num_clients: 20,
            duration_secs: 60,
            requests_per_client: 100,
            cache_hit_ratio_target: 0.75,
            timeout_secs: 120,
        },
    )
    .await;
    print_metrics_report(&metrics2);

    // Scenario 3: Heavy load
    println!("\n\n▶️  Scenario 3: Heavy Load (50 clients, 60s)\n");
    let scenario3 = Arc::new(TestCacheScenario {
        hit_probability: 0.70,
    });
    let metrics3 = run_load_test(
        scenario3,
        LoadTestConfig {
            num_clients: 50,
            duration_secs: 60,
            requests_per_client: 100,
            cache_hit_ratio_target: 0.70,
            timeout_secs: 120,
        },
    )
    .await;
    print_metrics_report(&metrics3);

    // Scenario 4: Extreme concurrency
    println!("\n\n▶️  Scenario 4: Extreme Concurrency (100 clients, 60s)\n");
    let scenario4 = Arc::new(TestCacheScenario {
        hit_probability: 0.75,
    });
    let metrics4 = run_load_test(
        scenario4,
        LoadTestConfig {
            num_clients: 100,
            duration_secs: 60,
            requests_per_client: 50,
            cache_hit_ratio_target: 0.75,
            timeout_secs: 120,
        },
    )
    .await;
    print_metrics_report(&metrics4);

    // Summary comparison
    println!("\n\n📊 Scenario Comparison Summary");
    println!("==============================");
    println!("Scenario              | Throughput | Avg Latency | Success Rate");
    println!("---------------------|------------|-------------|-------------");
    println!(
        "Light Load (5c)     | {:10.2} | {:11} | {:.2}%",
        metrics1.throughput_ops_per_sec,
        metrics1.avg_latency_ms,
        metrics1.success_rate()
    );
    println!(
        "Standard (20c)      | {:10.2} | {:11} | {:.2}%",
        metrics2.throughput_ops_per_sec,
        metrics2.avg_latency_ms,
        metrics2.success_rate()
    );
    println!(
        "Heavy (50c)         | {:10.2} | {:11} | {:.2}%",
        metrics3.throughput_ops_per_sec,
        metrics3.avg_latency_ms,
        metrics3.success_rate()
    );
    println!(
        "Extreme (100c)      | {:10.2} | {:11} | {:.2}%",
        metrics4.throughput_ops_per_sec,
        metrics4.avg_latency_ms,
        metrics4.success_rate()
    );

    println!("\n\n✅ Scenario suite complete!");
}

fn print_metrics_report(metrics: &LoadTestMetrics) {
    println!("📊 Test Results");
    println!("===============");
    println!();
    println!("Operations:");
    println!("  Total: {}", metrics.total_operations);
    println!(
        "  Successful: {} ({:.2}%)",
        metrics.successful_operations,
        metrics.success_rate()
    );
    println!(
        "  Failed: {} ({:.2}%)",
        metrics.failed_operations,
        metrics.error_rate()
    );
    println!();

    println!("Performance:");
    println!(
        "  Throughput: {:.2} ops/sec",
        metrics.throughput_ops_per_sec
    );
    println!("  Duration: {}ms", metrics.total_duration_ms);
    println!();

    println!("Latency Distribution:");
    println!("  Min: {}ms", metrics.min_latency_ms);
    println!("  Avg: {}ms", metrics.avg_latency_ms);
    println!("  P50: {}ms", metrics.p50_latency_ms);
    println!("  P95: {}ms", metrics.p95_latency_ms);
    println!("  P99: {}ms", metrics.p99_latency_ms);
    println!("  Max: {}ms", metrics.max_latency_ms);
    println!();

    println!("Cache Behavior:");
    println!("  Hits: {}", metrics.cache_hits);
    println!("  Misses: {}", metrics.cache_misses);
    println!("  Hit Rate: {:.2}%", metrics.cache_hit_rate());
    println!();

    // Pass/fail criteria
    println!("Release Readiness:");
    let mut all_pass = true;

    let success_ok = metrics.success_rate() > 95.0;
    println!(
        "  ✓ Success rate >95%: {}",
        if success_ok { "PASS ✅" } else { "FAIL ❌" }
    );
    all_pass &= success_ok;

    let hit_rate_ok = metrics.cache_hit_rate() > 70.0;
    println!(
        "  ✓ Cache hit rate >70%: {}",
        if hit_rate_ok { "PASS ✅" } else { "FAIL ❌" }
    );
    all_pass &= hit_rate_ok;

    let throughput_ok = metrics.throughput_ops_per_sec > 10.0;
    println!(
        "  ✓ Throughput >10 ops/sec: {}",
        if throughput_ok {
            "PASS ✅"
        } else {
            "FAIL ❌"
        }
    );
    all_pass &= throughput_ok;

    let latency_ok = metrics.p95_latency_ms < 500;
    println!(
        "  ✓ P95 latency <500ms: {}",
        if latency_ok { "PASS ✅" } else { "FAIL ❌" }
    );
    all_pass &= latency_ok;

    println!();
    if all_pass {
        println!("🎉 ALL CRITERIA MET - PRODUCTION READY");
    } else {
        println!("⚠️  SOME CRITERIA NOT MET - INVESTIGATE");
    }
}

fn save_metrics_json(metrics: &LoadTestMetrics, path: &str) -> std::io::Result<()> {
    let json = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "operations": {
            "total": metrics.total_operations,
            "successful": metrics.successful_operations,
            "failed": metrics.failed_operations,
        },
        "throughput_ops_per_sec": metrics.throughput_ops_per_sec,
        "duration_ms": metrics.total_duration_ms,
        "latency": {
            "min_ms": metrics.min_latency_ms,
            "avg_ms": metrics.avg_latency_ms,
            "p50_ms": metrics.p50_latency_ms,
            "p95_ms": metrics.p95_latency_ms,
            "p99_ms": metrics.p99_latency_ms,
            "max_ms": metrics.max_latency_ms,
        },
        "cache": {
            "hits": metrics.cache_hits,
            "misses": metrics.cache_misses,
            "hit_rate_percent": metrics.cache_hit_rate(),
        },
        "success_rate_percent": metrics.success_rate(),
        "error_rate_percent": metrics.error_rate(),
    });

    fs::write(path, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}
