#!/bin/bash

# MemoBuild Production Stack Startup Script
# This script starts the full production stack with all services

set -e

echo "🚀 Starting MemoBuild Production Stack..."

# Check if .env file exists
if [ ! -f .env ]; then
    echo "📋 Creating .env file from template..."
    cp .env.example .env
    echo "⚠️  Please edit .env file with your configuration before running again"
    exit 1
fi

# Check Docker and Docker Compose
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker first."
    exit 1
fi

if ! command -v docker-compose &> /dev/null; then
    echo "❌ Docker Compose is not installed. Please install Docker Compose first."
    exit 1
fi

# Create necessary directories
echo "📁 Creating necessary directories..."
mkdir -p monitoring/prometheus
mkdir -p monitoring/grafana/provisioning/datasources
mkdir -p monitoring/grafana/provisioning/dashboards
mkdir -p monitoring/grafana/dashboards
mkdir -p scripts/db

# Build and start services
echo "🔨 Building MemoBuild server image..."
docker-compose -f docker-compose.prod.yml build

echo "🚀 Starting all services..."
docker-compose -f docker-compose.prod.yml up -d

# Wait for services to be ready
echo "⏳ Waiting for services to be ready..."
sleep 30

# Check service health
echo "🔍 Checking service health..."

# Check PostgreSQL
echo "📊 Checking PostgreSQL..."
until docker exec memobuild-postgres pg_isready -U memobuild -d memobuild; do
    echo "Waiting for PostgreSQL..."
    sleep 5
done

# Check Redis
echo "📦 Checking Redis..."
until docker exec memobuild-redis redis-cli ping; do
    echo "Waiting for Redis..."
    sleep 5
done

# Check MinIO
echo "🗄️  Checking MinIO..."
until curl -f http://localhost:9000/minio/health/live; do
    echo "Waiting for MinIO..."
    sleep 5
done

# Check MemoBuild nodes
echo "🏗️  Checking MemoBuild nodes..."
for i in {1..3}; do
    node="memobuild-node$i"
    until docker exec $node memobuild health; do
        echo "Waiting for $node..."
        sleep 5
    done
done

echo "✅ All services are ready!"
echo ""
echo "🌐 Service URLs:"
echo "  • MemoBuild Node 1: http://localhost:9090"
echo "  • MemoBuild Node 2: http://localhost:9091"
echo "  • MemoBuild Node 3: http://localhost:9092"
echo "  • PostgreSQL: localhost:5432"
echo "  • Redis: localhost:6379"
echo "  • MinIO Console: http://localhost:9001 (admin/minioadmin)"
echo "  • Prometheus: http://localhost:9090"
echo "  • Grafana: http://localhost:3000 (admin/admin)"
echo "  • Jaeger: http://localhost:16686"
echo ""
echo "🔧 Useful commands:"
echo "  • View logs: docker-compose -f docker-compose.prod.yml logs -f [service]"
echo "  • Stop stack: docker-compose -f docker-compose.prod.yml down"
echo "  • Restart service: docker-compose -f docker-compose.prod.yml restart [service]"
echo ""
echo "📚 Next steps:"
echo "  1. Open Grafana to view dashboards"
echo "  2. Test MemoBuild with: memobuild build . --remote-exec"
echo "  3. Check cluster status: curl http://localhost:9090/cluster/status"
