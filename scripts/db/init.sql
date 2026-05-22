-- MemoBuild PostgreSQL initialization script

-- Create extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_stat_statements";

-- Cache entries table
CREATE TABLE IF NOT EXISTS cache_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    cache_key VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    artifact_path TEXT,
    size BIGINT DEFAULT 0,
    node_id VARCHAR(100),
    INDEX idx_cache_key (cache_key),
    INDEX idx_created_at (created_at),
    INDEX idx_node_id (node_id)
);

-- Build events table
CREATE TABLE IF NOT EXISTS build_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    build_id VARCHAR(255) NOT NULL,
    node_id VARCHAR(100),
    event_type VARCHAR(50) NOT NULL,
    event_data JSONB,
    timestamp TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    INDEX idx_build_id (build_id),
    INDEX idx_timestamp (timestamp),
    INDEX idx_node_id (node_id)
);

-- DAG entries table
CREATE TABLE IF NOT EXISTS dag_entries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    build_id VARCHAR(255) NOT NULL UNIQUE,
    dag_data JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    INDEX idx_build_id (build_id),
    INDEX idx_created_at (created_at)
);

-- Cluster nodes table
CREATE TABLE IF NOT EXISTS cluster_nodes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    node_id VARCHAR(100) NOT NULL UNIQUE,
    address VARCHAR(255) NOT NULL,
    weight INTEGER DEFAULT 1,
    region VARCHAR(100),
    is_healthy BOOLEAN DEFAULT true,
    last_heartbeat TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    cache_size BIGINT DEFAULT 0,
    connections INTEGER DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    INDEX idx_node_id (node_id),
    INDEX idx_healthy (is_healthy),
    INDEX idx_last_heartbeat (last_heartbeat)
);

-- Analytics table
CREATE TABLE IF NOT EXISTS build_analytics (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    build_id VARCHAR(255) NOT NULL,
    node_id VARCHAR(100),
    dirty_tasks INTEGER DEFAULT 0,
    cached_tasks INTEGER DEFAULT 0,
    duration_ms BIGINT DEFAULT 0,
    timestamp TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    INDEX idx_build_id (build_id),
    INDEX idx_timestamp (timestamp),
    INDEX idx_node_id (node_id)
);

-- API tokens table (for future authentication)
CREATE TABLE IF NOT EXISTS api_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    permissions JSONB DEFAULT '[]',
    is_active BOOLEAN DEFAULT true,
    expires_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    last_used_at TIMESTAMP WITH TIME ZONE,
    INDEX idx_token_hash (token_hash),
    INDEX idx_active (is_active)
);

-- Insert default admin token (password: admin)
INSERT INTO api_tokens (token_hash, name, permissions)
VALUES (
    '$argon2id$v=19$m=65536,t=3,p=4$c2FsdFzZWFkZXJzYWx0$hashplaceholder',
    'admin',
    '["*"]'
) ON CONFLICT (token_hash) DO NOTHING;

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS idx_cache_entries_node_created ON cache_entries(node_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_build_events_type_timestamp ON build_events(event_type, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_analytics_node_timestamp ON build_analytics(node_id, timestamp DESC);

-- Set up auto-vacuum tuning for better performance
ALTER TABLE cache_entries SET (autovacuum_vacuum_scale_factor = 0.1);
ALTER TABLE build_events SET (autovacuum_vacuum_scale_factor = 0.1);
ALTER TABLE dag_entries SET (autovacuum_vacuum_scale_factor = 0.1);
ALTER TABLE build_analytics SET (autovacuum_vacuum_scale_factor = 0.1);

-- Create view for cluster status
CREATE OR REPLACE VIEW cluster_status AS
SELECT 
    node_id,
    address,
    is_healthy,
    last_heartbeat,
    cache_size,
    connections,
    CASE 
        WHEN last_heartbeat > NOW() - INTERVAL '30 seconds' THEN 'online'
        WHEN last_heartbeat > NOW() - INTERVAL '5 minutes' THEN 'degraded'
        ELSE 'offline'
    END as status
FROM cluster_nodes
ORDER BY node_id;

-- Create view for build statistics
CREATE OR REPLACE VIEW build_stats AS
SELECT 
    DATE(timestamp) as build_date,
    COUNT(*) as total_builds,
    AVG(duration_ms) as avg_duration_ms,
    SUM(dirty_tasks) as total_dirty_tasks,
    SUM(cached_tasks) as total_cached_tasks,
    ROUND(SUM(cached_tasks)::decimal / (SUM(dirty_tasks) + SUM(cached_tasks)) * 100, 2) as cache_hit_rate_percent
FROM build_analytics
GROUP BY DATE(timestamp)
ORDER BY build_date DESC;

-- Grant permissions (adjust user as needed)
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO memobuild;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO memobuild;
GRANT SELECT ON ALL VIEWS IN SCHEMA public TO memobuild;
