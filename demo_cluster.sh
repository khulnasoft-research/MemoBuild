#!/bin/bash
# MemoBuild Clustered Cache Demo
# This script demonstrates the distributed cache cluster with auto-scaling

set -e

echo "🏗️ MemoBuild Clustered Cache Demo"
echo "=================================="

# Build the project with cluster features
echo "📦 Building MemoBuild with cluster features..."
cargo build --release

# Start cluster node 1
echo "🏗️ Starting Cluster Node 1 on port 9090..."
./target/release/memobuild cluster --port 9090 --node-id node1 &
NODE1_PID=$!

# Start cluster node 2
echo "🏗️ Starting Cluster Node 2 on port 9091..."
./target/release/memobuild cluster --port 9091 --node-id node2 --peers http://localhost:9090 &
NODE2_PID=$!

# Start cluster node 3
echo "🏗️ Starting Cluster Node 3 on port 9092..."
./target/release/memobuild cluster --port 9092 --node-id node3 --peers http://localhost:9090,http://localhost:9091 &
NODE3_PID=$!

# Wait for nodes to start
sleep 5

# Check cluster status
echo "📊 Checking cluster status..."
curl -s http://localhost:9090/cluster/status | jq .

# Add nodes to cluster via API
echo "➕ Adding node2 to cluster via API..."
curl -X POST http://localhost:9090/cluster/nodes \
  -H "Content-Type: application/json" \
  -d '{"id": "node2", "address": "http://localhost:9091", "weight": 100}' | jq .

echo "➕ Adding node3 to cluster via API..."
curl -X POST http://localhost:9090/cluster/nodes \
  -H "Content-Type: application/json" \
  -d '{"id": "node3", "address": "http://localhost:9092", "weight": 100}' | jq .

# Check updated cluster status
echo "📊 Checking updated cluster status..."
curl -s http://localhost:9090/cluster/status | jq .

# Test distributed caching
echo "💾 Testing distributed caching..."
echo "   Putting artifact to node1..."
echo "test data for distributed cache" | curl -X PUT http://localhost:9090/cache/test-hash \
  -H "Content-Type: application/octet-stream" \
  --data-binary @-

echo "   Getting artifact from node2 (should be replicated)..."
curl -s http://localhost:9091/cache/test-hash

echo "   Getting artifact from node3 (should be replicated)..."
curl -s http://localhost:9092/cache/test-hash

# Check scaling status
echo "⚖️ Checking auto-scaling status..."
curl -s http://localhost:9090/scaling/status | jq .

# Record some scaling metrics
echo "📈 Recording scaling metrics..."
curl -X POST http://localhost:9090/scaling/metrics \
  -H "Content-Type: application/json" \
  -d '{
    "timestamp": "'$(date +%s)'",
    "active_builds": 5,
    "queued_builds": 2,
    "worker_utilization": 0.75,
    "cache_hit_rate": 0.85,
    "avg_build_time_ms": 45000,
    "memory_usage_mb": 1024,
    "cpu_usage_percent": 65.0
  }' | jq .

# Check scaling prediction
echo "🔮 Checking resource prediction..."
curl -s "http://localhost:9090/scaling/predict?time_window_secs=3600" | jq .

# Cleanup
echo "🧹 Cleaning up..."
kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null || true

echo "✅ Cluster demo completed successfully!"