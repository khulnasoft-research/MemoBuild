# 🚀 MemoBuild Scalability Testing Guide

**Last Updated:** February 21, 2026  
**Version:** 0.2.0+  
**Status:** Production-Ready

---

## 📋 Table of Contents

1. [Overview](#overview)
2. [Test Framework](#test-framework)
3. [Running Tests](#running-tests)
4. [Test Scenarios](#test-scenarios)
5. [Performance Metrics](#performance-metrics)
6. [Release Criteria](#release-criteria)
7. [CI/CD Integration](#cicd-integration)
8. [Troubleshooting](#troubleshooting)

---

## 🎯 Overview

MemoBuild's scalability testing suite verifies performance under:
- High concurrency (up to 200+ concurrent clients)
- Sustained load (60+ second tests)
- Varying cache hit rates
- Extreme stress conditions

**Goal:** Confirm production-readiness through comprehensive performance validation.

---

## 🧪 Test Framework

### Architecture

```
LoadTestFramework
├── LoadTestState        → Shared metrics collection
├── LoadTestMetrics      → Results aggregation
├── LoadTestScenario     → Custom test behavior
└── LoadTestConfig       → Test parameters
```

### Core Components

#### 1. **LoadTestState**
Captures real-time metrics with atomic operations:
```rust
pub struct LoadTestState {
    pub total_ops: Arc<AtomicU64>,
    pub successful_ops: Arc<AtomicU64>,
    pub cache_hits: Arc<AtomicU64>,
    pub latencies: Arc<Mutex<Vec<u64>>>,
    // ... more metrics
}
```

#### 2. **LoadTestMetrics**
Comprehensive result analysis:
```rust
pub struct LoadTestMetrics {
    pub total_operations: u64,
    pub success_rate: f64,
    pub throughput_ops_per_sec: f64,
    pub p50_latency_ms: u64,
    pub p95_latency_ms: u64,
    pub p99_latency_ms: u64,
    pub cache_hit_rate: f64,
    // ... more fields
}
```

#### 3. **LoadTestScenario** (Trait)
Customizable test operations:
```rust
#[async_trait]
pub trait LoadTestScenario: Send + Sync {
    async fn execute(&self) -> (bool, bool); // (success, cache_hit)
    fn name(&self) -> &'static str;
}
```

#### 4. **LoadTestConfig**
Parameterized test configuration:
```rust
pub struct LoadTestConfig {
    pub num_clients: usize,        // Concurrent clients
    pub duration_secs: u64,        // Test duration
    pub requests_per_client: usize,// Operations per client
    pub cache_hit_ratio_target: f64,
    pub timeout_secs: u64,
}
```

---

## ▶️ Running Tests

### 1. Unit Tests (Integrated)

```bash
# Run all scalability tests
cargo test --test scalability_test -- --nocapture

# Specific scenario
cargo test test_concurrent_cache_operations -- --nocapture

# With logging
RUST_LOG=debug cargo test --test scalability_test -- --nocapture
```

**Duration:** ~5-10 minutes for full suite

### 2. Load Test Runner (Example)

**Single test configuration:**
```bash
# Run with defaults (10 clients, 60s)
cargo run --example load_test_runner --release

# Custom parameters
cargo run --example load_test_runner --release -- \
  --clients 50 \
  --duration 120 \
  --requests 100 \
  --hit-rate 0.75 \
  --output results.json
```

**Scenario suite:**
```bash
# Run all predefined scenarios
cargo run --example load_test_runner --release -- --scenarios

# Output to file
cargo run --example load_test_runner --release -- \
  --scenarios \
  --output scalability_report.json
```

**Parameters:**
- `--clients N` → Number of concurrent clients (default: 10)
- `--duration S` → Test duration in seconds (default: 60)
- `--requests N` → Requests per client (default: 100)
- `--hit-rate F` → Cache hit probability 0.0-1.0 (default: 0.75)
- `--output FILE` → Save results to JSON file
- `--scenarios` → Run predefined scenario suite

---

## 🔬 Test Scenarios

### Scenario 1: Light Load Baseline
**Configuration:**
- Clients: 5
- Duration: 30s
- Requests/client: 50
- Cache hit rate: 80%

**Purpose:** Establish performance baseline  
**Expected:** Low latency, high throughput

```bash
cargo run --example load_test_runner --release -- \
  --clients 5 --duration 30 --requests 50 --hit-rate 0.80
```

### Scenario 2: Standard Production Load
**Configuration:**
- Clients: 20
- Duration: 60s
- Requests/client: 100
- Cache hit rate: 75%

**Purpose:** Typical production workload  
**Expected:** Balanced latency/throughput, stable metrics

```bash
cargo run --example load_test_runner --release -- \
  --clients 20 --duration 60 --requests 100 --hit-rate 0.75
```

### Scenario 3: Heavy Production Load
**Configuration:**
- Clients: 50
- Duration: 60s
- Requests/client: 100
- Cache hit rate: 70%

**Purpose:** Peak load testing  
**Expected:** Maintains SLO under stress

```bash
cargo run --example load_test_runner --release -- \
  --clients 50 --duration 60 --requests 100 --hit-rate 0.70
```

### Scenario 4: Extreme Concurrency
**Configuration:**
- Clients: 100-200
- Duration: 60s
- Requests/client: 50
- Cache hit rate: 75%

**Purpose:** Failure mode analysis  
**Expected:** Graceful degradation, error <5%

```bash
cargo run --example load_test_runner --release -- \
  --clients 100 --duration 60 --requests 50 --hit-rate 0.75
```

### Scenario 5: Sustained Load
**Configuration:**
- Clients: 20
- Duration: 300s (5 minutes)
- Requests/client: 30
- Cache hit rate: 75%

**Purpose:** Stability over extended period  
**Expected:** No memory leaks, consistent latency

```bash
cargo run --example load_test_runner --release -- \
  --clients 20 --duration 300 --requests 30 --hit-rate 0.75
```

---

## 📊 Performance Metrics

### Key Metrics

| Metric | Meaning | Target | Action |
|--------|---------|--------|--------|
| **Throughput** | Operations/sec | >20 ops/sec | Increase if <10 |
| **Avg Latency** | Mean response time | <100ms | Investigate if >200ms |
| **P95 Latency** | 95th percentile | <500ms | Investigate if >1s |
| **P99 Latency** | 99th percentile | <1s | Investigate if >3s |
| **Success Rate** | % successful ops | >95% | Critical if <90% |
| **Cache Hit Rate** | % cache hits | >70% | Tune if <60% |
| **Error Rate** | % failed ops | <5% | Critical if >10% |

### Interpretation

```
✅ PASS Criteria:
  - Success rate > 95%
  - Throughput > 20 ops/sec
  - P95 latency < 500ms
  - Cache hit rate > 70%
  - Error rate < 5%

⚠️  INVESTIGATE:
  - Success rate 90-95%
  - Throughput 10-20 ops/sec
  - P95 latency 500-1000ms
  - Cache hit rate 60-70%
  - Error rate 5-10%

❌ FAIL Criteria:
  - Success rate < 90%
  - Throughput < 10 ops/sec
  - P95 latency > 1s
  - Cache hit rate < 60%
  - Error rate > 10%
```

---

## 🎯 Release Criteria

### Pre-v0.2.0 Release Checklist

- [ ] Light load test passes (5 clients, 80% hit rate)
- [ ] Standard load test passes (20 clients, 75% hit rate)
- [ ] Heavy load test passes (50 clients, 70% hit rate)
- [ ] Extreme concurrency test passes (100 clients)
- [ ] Sustained load test (5 min) shows no degradation
- [ ] P95 latency < 500ms under all scenarios
- [ ] Success rate > 95% under all scenarios
- [ ] Cache hit rate achieves targets (±5%)

### Pre-v1.0.0 Targets

- [ ] Support 500+ concurrent clients
- [ ] Achieve >50,000 ops/sec throughput
- [ ] P99 latency < 500ms
- [ ] Cache hit rate > 85%
- [ ] Zero-downtime deployment tested
- [ ] Horizontal scaling verified
- [ ] Load balancer integration tested

---

## 🔄 CI/CD Integration

### GitHub Actions Workflow

```yaml
name: Scalability Tests
on: [push, pull_request]

jobs:
  scalability:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run scalability tests
        run: cargo test --test scalability_test --release -- --nocapture
      - name: Run load test (light)
        run: cargo run --example load_test_runner --release -- \
          --clients 10 --duration 30 --output light_test.json
      - name: Upload results
        uses: actions/upload-artifact@v3
        with:
          name: scalability-results
          path: "*.json"
```

### Performance Regression Detection

```bash
# Establish baseline
cargo run --example load_test_runner --release -- --output baseline.json

# Run new version
cargo run --example load_test_runner --release -- --output current.json

# Compare (Python script)
python3 compare_results.py baseline.json current.json
```

---

## 📈 Analyzing Results

### JSON Output Format

```json
{
  "timestamp": "2026-02-21T10:00:00Z",
  "operations": {
    "total": 10500,
    "successful": 10020,
    "failed": 480
  },
  "throughput_ops_per_sec": 175.0,
  "latency": {
    "min_ms": 1,
    "avg_ms": 45,
    "p50_ms": 42,
    "p95_ms": 120,
    "p99_ms": 250,
    "max_ms": 1200
  },
  "cache": {
    "hits": 7875,
    "misses": 2625,
    "hit_rate_percent": 75.0
  }
}
```

### Plotting Results

```bash
# Install requirements
pip install matplotlib pandas

# Plot metrics over time
python3 plot_scalability.py results.json
```

---

## 🔧 Troubleshooting

### High Latency Under Load

**Symptoms:** P95 latency >500ms, increasing with clients

**Diagnose:**
```bash
# Check resource usage
top -p $(pgrep -f load_test_runner)
# Look for: CPU >95%, Memory growing

# Run with fewer clients
cargo run --example load_test_runner --release -- --clients 5
```

**Solutions:**
1. Increase thread pool size: `worker_threads = 8`
2. Enable release optimizations: `--release`
3. Check for lock contention in logs
4. Profile with flamegraph

### Low Cache Hit Rate

**Symptoms:** Cache hit rate <60%, inconsistent across runs

**Diagnose:**
```bash
# Verify scenario configuration
cargo run --example load_test_runner --release -- \
  --hit-rate 0.75 --output debug.json
# Check: hit_rate_percent in output
```

**Solutions:**
1. Verify `--hit-rate` parameter matches scenario
2. Check cache implementation for correctness
3. Ensure cache persistence across operations

### Out of Memory

**Symptoms:** Process killed, latency tracking uses too much RAM

**Solution:** Reduce operations or run in smaller batches
```bash
cargo run --example load_test_runner --release -- \
  --clients 5 --duration 30 --requests 50
```

### Timeout Errors

**Symptoms:** Operations timeout after configured duration

**Solution:** Increase `--duration` or reduce per-client requests
```bash
cargo run --example load_test_runner --release -- \
  --clients 10 --duration 120 --requests 50
```

---

## 📚 Reference

### Entry Points

- **Test Framework:** `src/loadtest.rs`
- **Integration Tests:** `tests/scalability_test.rs`
- **Example Runner:** `examples/load_test_runner.rs`
- **This Guide:** `docs/SCALABILITY_TESTING.md`

### Key Files

```
MemoBuild/
├── src/loadtest.rs              # Core testing framework
├── tests/scalability_test.rs    # Integration tests
├── examples/load_test_runner.rs # Standalone runner
├── docs/SCALABILITY_TESTING.md  # This guide
└── scripts/
    └── compare_results.py       # Result comparison tool
```

---

## 🚀 Quick Start

```bash
# 1. Build in release mode
cargo build --release

# 2. Run light test (baseline)
cargo run --example load_test_runner --release

# 3. Run standard test (20 clients, 60s)
cargo run --example load_test_runner --release -- \
  --clients 20 --duration 60

# 4. Run all scenarios with results
cargo run --example load_test_runner --release -- \
  --scenarios --output results.json

# 5. Analyze results
cat results.json | jq '.operations, .latency, .cache'
```

---

**For questions about scalability testing, see:** `src/loadtest.rs` module docs

**Report performance issues:** Create GitHub issue with `--output` JSON results

**Next Steps After Scalability Testing:**
1. ✅ Scalability Testing (THIS)
2. ⏭️ Documentation (API, Deployment, Troubleshooting)
3. ⏭️ API Versioning & Stability
4. ⏭️ Performance Benchmarking (continuous regression detection)
