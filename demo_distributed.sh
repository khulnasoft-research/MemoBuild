#!/bin/bash
# MemoBuild Distributed Execution Demo
# This script demonstrates the distributed build system

set -e

echo "🚀 MemoBuild Distributed Execution Demo"
echo "========================================"

# Build the project with remote-exec feature
echo "📦 Building MemoBuild with remote-exec feature..."
cargo build --release --features remote-exec

# Start the scheduler in background
echo "📡 Starting scheduler on port 9000..."
./target/release/memobuild scheduler --port 9000 &
SCHEDULER_PID=$!

# Wait for scheduler to start
sleep 2

# Start worker 1 in background
echo "👷 Starting worker 1 on port 9001..."
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 ./target/release/memobuild worker --port 9001 --scheduler-url http://localhost:9000 &
WORKER1_PID=$!

# Start worker 2 in background
echo "👷 Starting worker 2 on port 9002..."
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 ./target/release/memobuild worker --port 9002 --scheduler-url http://localhost:9000 &
WORKER2_PID=$!

# Wait for workers to register
sleep 3

# Check registered workers
echo "📋 Checking registered workers..."
curl -s http://localhost:9000/workers | jq .

# Run a build with remote execution
echo "🔨 Running build with remote execution..."
cd examples/nodejs-app
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 ../../../target/release/memobuild build . --remote-exec

# Cleanup
echo "🧹 Cleaning up..."
kill $WORKER1_PID $WORKER2_PID $SCHEDULER_PID 2>/dev/null || true

echo "✅ Demo completed successfully!"