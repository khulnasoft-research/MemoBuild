# 🧠 MemoBuild Engine

[![CI](https://github.com/nrelab/MemoBuild/actions/workflows/ci.yml/badge.svg)](https://github.com/nrelab/MemoBuild/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust: 1.75+](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org/)
[![Version: 0.4.0](https://img.shields.io/badge/version-0.4.0-green.svg)]()
[![Enterprise Ready](https://img.shields.io/badge/enterprise-ready-blue.svg)]()

**Enterprise-Grade Distributed Build System with Smart Caching & Auto-Scaling**

MemoBuild is a next-generation build system that intelligently rebuilds only what's changed, using advanced dependency tracking, multi-layer caching, and OCI-compatible image generation. Now featuring **enterprise-grade high availability, automatic scaling, and distributed caching**.

**[🚀 Read the Vision](./docs/VISION.md)** | **[📄 Technical Whitepaper](./docs/WHITEPAPER.md)** | **[💻 CLI Manual](./docs/CLI_REFERENCE.md)** | **[🌐 CI/CD Integration](./docs/guides/CI_CD_INTEGRATION.md)** | **[🏗️ Cluster Setup](./docs/releases/PHASE_2_COMPLETE.md)**

---

## 🏗️ Architecture Overview

![Architecture](./docs/ARCHITECTURE.svg)

MemoBuild transforms container builds from **linear execution → dependency graph execution** with **enterprise-grade distributed caching and auto-scaling**.

---

## 🚀 Quick Start

### 1. Installation

```bash
# Clone the repository
git clone https://github.com/nrelab/MemoBuild.git
cd memobuild

# Build and install locally
cargo install --path .
```

### 2. Basic Usage

```bash
# Build current directory
memobuild build .

# Visualize the build graph
memobuild graph

# Explain why a node was or wasn't cached
memobuild explain-cache

# Build and push to registry
export MEMOBUILD_REGISTRY=ghcr.io
export MEMOBUILD_REPO=myuser/app
export MEMOBUILD_TOKEN=$(gh auth token)
memobuild build --push .
```

### 3. Distributed Cache (Enterprise)

```bash
# Start a clustered cache server (single node)
memobuild cluster --port 9090 --node-id node1

# Start additional cluster nodes
memobuild cluster --port 9091 --node-id node2 --peers http://localhost:9090
memobuild cluster --port 9092 --node-id node3 --peers http://localhost:9090

# Client: Connect to distributed cache
export MEMOBUILD_REMOTE_URL=http://localhost:9090
memobuild build .
```

### 4. Kubernetes Auto-Scaling (Production)

```bash
# Deploy with auto-scaling enabled
helm install memobuild-cluster ./charts/memobuild-cluster \
  --set replicaCount=3 \
  --set autoscaling.enabled=true \
  --set postgresql.enabled=true
```

---

## 📂 Examples

Visit the [examples/](./examples) directory to see ready-to-use projects:
- **[Node.js App](./examples/nodejs-app)**: Simple web server with dependency caching.
- **[Rust App](./examples/rust-app)**: High-performance async app showing complex build caching.
- **[Cluster Demo](./demo_cluster.sh)**: Multi-node distributed cache setup.

---

## 📋 Documentation Reference

📚 **[Full Documentation Index](./docs/README.md)**

- **[Vision](./docs/VISION.md)**: The philosophy and problem statement.
- **[Whitepaper](./docs/WHITEPAPER.md)**: Deep technical spec and mathematical foundations.
- **[CLI Reference](./docs/CLI_REFERENCE.md)**: Detailed command and option manual.
- **[Architecture](./docs/ARCHITECTURE.md)**: System design overview.
- **[Contributing](./docs/development/CONTRIBUTING.md)**: How to contribute.
- **[Quick Reference](./docs/guides/QUICK_REFERENCE.md)**: Fast command reference.

---

## ✨ Features

### Core Features
- **BLAKE3 Hashing**: Ultra-fast content hashing for change detection.
- **Tiered Smart Cache**: Multi-layer (In-memory, Local, Remote, Distributed) sharing.
- **DAG Execution**: Parallelized rebuild of affected subgraphs only.
- **OCI Compliance**: Push directly to any standard container registry.
- **K8s Helper**: Generate native Kubernetes Job manifests for cloud builds.

### 🚀 Enterprise Features (Phase 2)
- **Multi-Master Cache Clustering**: Consistent hashing with automatic replication
- **PostgreSQL Database Scaling**: Connection pooling with read replicas
- **Kubernetes Auto-Scaling**: HPA integration with predictive scaling
- **Fault-Tolerant Architecture**: Zero-downtime node failures
- **Enterprise Monitoring**: Comprehensive metrics and health checks
- **Global Distribution**: Multi-region deployment support

---

## 📋 Core Components

### 1. **Change Detector** (`src/core.rs`)
- BLAKE3-based file hashing
- Directory tree hashing
- Dependency-aware hash computation
- Dirty flag propagation

### 2. **Graph Builder** (`src/graph.rs`)
- Dockerfile → DAG conversion
- Node types: Source, Build, Artifact, Dependency
- Topological sorting
- Dependency management

### 3. **Hybrid Cache System** (`src/cache.rs` + `src/cache_cluster.rs`)
- **Tiered caching**: L1 In-memory, L2 Local, L3 Remote, L4 Distributed
- **Content-addressed storage**: CAS with consistent hashing
- **Multi-master replication**: Automatic data replication across nodes
- **Gzip compression**: Optimized artifact storage

### 4. **OCI Image Exporter** (`src/oci/mod.rs`)
- OCI-compliant manifest and config generation
- Layer digest calculation
- Registry push/pull using Distribution Spec

### 5. **Distributed Cache Server** (`src/cluster_server.rs`)
- REST API for cluster management and auto-scaling
- Health monitoring and metrics collection
- Dynamic node registration and failover
- Kubernetes HPA integration

### 6. **Database Scaling** (`src/scalable_db.rs`)
- PostgreSQL with connection pooling
- Read replica distribution
- Schema migrations and optimization
- Async operations with tokio

### 7. **Auto-Scaling Engine** (`src/auto_scaling.rs`)
- Kubernetes HPA integration
- Queue-based scaling triggers
- Predictive scaling algorithms
- Resource utilization monitoring

---

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_end_to_end_build_with_remote_cache

# Test cluster functionality
cargo test cache_cluster
cargo test scalable_db
cargo test auto_scaling
```

---

## 🚀 Production Deployment

### Kubernetes with Auto-Scaling

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: memobuild-cluster
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: memobuild
        image: memobuild:latest
        command: ["memobuild", "cluster", "--port", "9090", "--node-id", "$(NODE_ID)"]
        env:
        - name: NODE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: PEERS
          value: "http://memobuild-cluster-0:9090,http://memobuild-cluster-1:9090"
---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: memobuild-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: memobuild-cluster
  minReplicas: 3
  maxReplicas: 50
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
```

### Helm Chart Installation

```bash
# Add the repository
helm repo add memobuild https://nrelab.github.io/memobuild

# Install with PostgreSQL and auto-scaling
helm install memobuild-cluster memobuild/memobuild \
  --set replicaCount=5 \
  --set postgresql.enabled=true \
  --set autoscaling.enabled=true \
  --set autoscaling.minReplicas=3 \
  --set autoscaling.maxReplicas=50
```

---

## 📊 Performance & Scalability

| Metric | Single Node | 3-Node Cluster | 10-Node Cluster |
|--------|-------------|----------------|-----------------|
| **Concurrent Builds** | 50 | 500 | 2000+ |
| **Cache Throughput** | 100 ops/sec | 1000 ops/sec | 5000+ ops/sec |
| **Database Queries** | 500 qps | 5000 qps | 20000+ qps |
| **Fault Tolerance** | None | 1 node failure | 3 node failures |
| **Auto-Scaling** | Manual | Queue-based | Predictive |

---

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

MIT License - see [LICENSE](./LICENSE) file for details

---

**MemoBuild** - Enterprise-grade distributed builds, faster deployments 🚀
export MEMOBUILD_TOKEN=$(gh auth token)
memobuild build --push .
```

### 3. Remote Cache Sharing (Optional)

```bash
# Start the Remote Cache Server
memobuild server --port 8080 --storage ./cache-data

# Client: Share artifacts across the team
export MEMOBUILD_REMOTE_URL=http://localhost:8080
memobuild build .
```

---

## 📂 Examples

Visit the [examples/](./examples) directory to see ready-to-use projects:
- **[Node.js App](./examples/nodejs-app)**: Simple web server with dependency caching.
- **[Rust App](./examples/rust-app)**: High-performance async app showing complex build caching.

---

## 📋 Documentation Reference

- **[Vision](./docs/VISION.md)**: The philosophy and problem statement.
- **[Whitepaper](./docs/WHITEPAPER.md)**: Deep technical spec and mathematical foundations.
- **[CLI Reference](./docs/CLI_REFERENCE.md)**: Detailed command and option manual.
- **[Architecture Diagram](./docs/ARCHITECTURE.svg)**: Visual process flow.
- **[CI/CD Integration](./CI_CD_INTEGRATION.md)**: Blueprint for GitHub Actions, GitLab, and cloud runners.

---

## ✨ Features

- **BLAKE3 Hashing**: Ultra-fast content hashing for change detection.
- **Tiered Smart Cache**: Multi-layer (In-memory, Local, Remote) sharing.
- **DAG Execution**: Parallelized rebuild of affected subgraphs only.
- **OCI Compliance**: Push directly to any standard container registry.
- **K8s Helper**: Generate native Kubernetes Job manifests for cloud builds.

---

## 📋 Core Components

### 1. **Change Detector** (`src/core.rs`)
- BLAKE3-based file hashing
- Directory tree hashing
- Dependency-aware hash computation
- Dirty flag propagation

### 2. **Graph Builder** (`src/graph.rs`)
- Dockerfile → DAG conversion
- Node types: Source, Build, Artifact, Dependency
- Topological sorting
- Dependency management

### 3. **Hybrid Cache System** (`src/cache.rs`)
- Tiered caching (L1 In-memory, L2 Local, L3 Remote)
- Content-addressed artifact storage (CAS)
- Gzip compression for artifacts

### 4. **OCI Image Exporter** (`src/oci/mod.rs`)
- OCI-compliant manifest and config generation
- Layer digest calculation
- Registry push/pull using Distribution Spec

---

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_end_to_end_build_with_remote_cache
```

---

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

MIT License - see [LICENSE](./LICENSE) file for details

---

**MemoBuild** - Smart builds, faster deployments 🚀
