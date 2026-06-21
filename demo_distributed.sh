#!/bin/bash
# MemoBuild Distributed Execution Demo
# This script demonstrates the distributed build system

set -euo pipefail

echo "🚀 MemoBuild Distributed Execution Demo"
echo "========================================"

# Build the project with remote-exec feature
echo "📦 Building MemoBuild with remote-exec feature..."
cargo build --release --features remote-exec

# Ensure binary exists
BINARY=target/release/memobuild
if [ ! -x "${BINARY}" ]; then
	echo "❌ Binary ${BINARY} not found or not executable. Build failed?"
	exit 1
fi

cleanup() {
	echo "🧹 Cleaning up..."
	kill "${WORKER1_PID:-}" "${WORKER2_PID:-}" "${SCHEDULER_PID:-}" 2>/dev/null || true
}
trap cleanup EXIT

wait_for_http() {
	local url="$1"
	local retries=10
	local i
	for i in $(seq 1 "$retries"); do
		if curl -sSf "$url" >/dev/null 2>&1; then
			return 0
		fi
		sleep 1
	done
	return 1
}

# Start the scheduler in background
echo "📡 Starting scheduler on port 9000..."
"${BINARY}" scheduler --port 9000 &
SCHEDULER_PID=$!

# Wait for scheduler HTTP endpoint
if ! wait_for_http http://localhost:9000/; then
	echo "❌ Scheduler did not become ready on http://localhost:9000"
	exit 1
fi

# Start worker 1 in background
echo "👷 Starting worker 1 on port 9001..."
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 "${BINARY}" worker --port 9001 --scheduler-url http://localhost:9000 &
WORKER1_PID=$!

# Start worker 2 in background
echo "👷 Starting worker 2 on port 9002..."
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 "${BINARY}" worker --port 9002 --scheduler-url http://localhost:9000 &
WORKER2_PID=$!

# Wait for workers to register
echo "⏳ Waiting for workers to register..."
if ! wait_for_http http://localhost:9000/workers; then
	echo "❌ Workers did not register in time"
	exit 1
fi

# Check registered workers
echo "📋 Checking registered workers..."
curl -s http://localhost:9000/workers | jq .

# Run a build with remote execution
echo "🔨 Running build with remote execution..."
cd examples/nodejs-app
# correct relative path: from examples/nodejs-app -> ../../target/release/memobuild
MEMOBUILD_SCHEDULER_URL=http://localhost:9000 ../../target/release/memobuild build . --remote-exec

echo "✅ Demo completed successfully!"