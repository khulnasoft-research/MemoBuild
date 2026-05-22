//! Scalable Database Storage with PostgreSQL
//!
//! This module provides PostgreSQL-based metadata storage with connection pooling,
//! read replicas, and schema migrations for horizontal scaling.

use anyhow::Result;
use async_trait::async_trait;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod};
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub hash: String,
    pub artifact_path: String,
    pub size: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: chrono::DateTime<chrono::Utc>,
    pub hit_count: i32,
    pub is_layered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheLayer {
    pub layer_hash: String,
    pub storage_path: String,
    pub size: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: chrono::DateTime<chrono::Utc>,
    pub ref_count: i32,
}

/// Configuration for PostgreSQL connection
#[derive(Debug, Clone, Deserialize)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub max_connections: usize,
    pub min_idle: Option<usize>,
}

/// Scalable metadata store with PostgreSQL
pub struct PostgresMetadataStore {
    pool: Pool,
}

impl PostgresMetadataStore {
    pub async fn new(config: PostgresConfig) -> Result<Self> {
        let mut cfg = Config::new();
        cfg.host = Some(config.host);
        cfg.port = Some(config.port);
        cfg.dbname = Some(config.database);
        cfg.user = Some(config.user);
        cfg.password = Some(config.password);
        cfg.manager = Some(ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        });

        let pool = cfg.create_pool(None, NoTls)?;

        // Test connection and run migrations
        let client = pool.get().await?;
        Self::run_migrations(&client).await?;

        Ok(Self { pool })
    }

    async fn run_migrations(client: &deadpool_postgres::Client) -> Result<()> {
        // Create tables if they don't exist
        client
            .batch_execute(
                r#"
                CREATE TABLE IF NOT EXISTS cache_entries (
                    hash TEXT PRIMARY KEY,
                    artifact_path TEXT,
                    size BIGINT,
                    created_at TIMESTAMPTZ DEFAULT NOW(),
                    last_used TIMESTAMPTZ DEFAULT NOW(),
                    hit_count INT DEFAULT 0,
                    is_layered BOOLEAN DEFAULT FALSE
                );

                CREATE TABLE IF NOT EXISTS cache_layers (
                    layer_hash TEXT PRIMARY KEY,
                    storage_path TEXT,
                    size BIGINT,
                    created_at TIMESTAMPTZ DEFAULT NOW(),
                    last_used TIMESTAMPTZ DEFAULT NOW(),
                    ref_count INT DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS node_to_layers (
                    node_hash TEXT,
                    layer_hash TEXT,
                    position INT,
                    PRIMARY KEY(node_hash, position),
                    FOREIGN KEY(node_hash) REFERENCES cache_entries(hash) ON DELETE CASCADE,
                    FOREIGN KEY(layer_hash) REFERENCES cache_layers(layer_hash) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS auth_tokens (
                    id SERIAL PRIMARY KEY,
                    token_hash TEXT UNIQUE NOT NULL,
                    description TEXT,
                    created_at TIMESTAMPTZ DEFAULT NOW(),
                    active BOOLEAN DEFAULT TRUE
                );

                CREATE INDEX IF NOT EXISTS idx_auth_tokens_active ON auth_tokens(active);
                "#,
            )
            .await?;

        Ok(())
    }

    pub async fn insert(&self, hash: &str, path: &str, size: i64) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"
                INSERT INTO cache_entries (hash, artifact_path, size, created_at, last_used, hit_count, is_layered)
                VALUES ($1, $2, $3, NOW(), NOW(), 0, FALSE)
                ON CONFLICT(hash) DO UPDATE SET
                    last_used = NOW(),
                    hit_count = cache_entries.hit_count + 1
                "#,
                &[&hash, &path, &size],
            )
            .await?;
        Ok(())
    }

    pub async fn insert_layered_node(
        &self,
        hash: &str,
        size: i64,
        layer_hashes: &[String],
    ) -> Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        tx.execute(
            r#"
            INSERT INTO cache_entries (hash, artifact_path, size, created_at, last_used, hit_count, is_layered)
            VALUES ($1, '', $2, NOW(), NOW(), 0, TRUE)
            ON CONFLICT(hash) DO UPDATE SET
                last_used = NOW(),
                hit_count = cache_entries.hit_count + 1
            "#,
            &[&hash, &size],
        )
        .await?;

        // Remove old mappings
        tx.execute("DELETE FROM node_to_layers WHERE node_hash = $1", &[&hash])
            .await?;

        for (pos, layer_hash) in layer_hashes.iter().enumerate() {
            tx.execute(
                "INSERT INTO node_to_layers (node_hash, layer_hash, position) VALUES ($1, $2, $3)",
                &[&hash, &layer_hash, &(pos as i32)],
            )
            .await?;

            // Increment ref count for existing layers
            tx.execute(
                "UPDATE cache_layers SET ref_count = ref_count + 1 WHERE layer_hash = $1",
                &[&layer_hash],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn insert_layer(&self, hash: &str, path: &str, size: i64) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"
                INSERT INTO cache_layers (layer_hash, storage_path, size, created_at, last_used)
                VALUES ($1, $2, $3, NOW(), NOW())
                ON CONFLICT(layer_hash) DO UPDATE SET
                    last_used = NOW()
                "#,
                &[&hash, &path, &size],
            )
            .await?;
        Ok(())
    }

    pub async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT layer_hash FROM node_to_layers WHERE node_hash = $1 ORDER BY position",
                &[&hash],
            )
            .await?;

        let mut layers = Vec::new();
        for row in rows {
            layers.push(row.get(0));
        }

        if layers.is_empty() {
            // Check if node exists at all
            let count: i64 = client
                .query_one(
                    "SELECT COUNT(*) FROM cache_entries WHERE hash = $1",
                    &[&hash],
                )
                .await?
                .get(0);
            if count == 0 {
                return Ok(None);
            }
        }

        Ok(Some(layers))
    }

    pub async fn layer_exists(&self, hash: &str) -> Result<bool> {
        let client = self.pool.get().await?;
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM cache_layers WHERE layer_hash = $1",
                &[&hash],
            )
            .await?
            .get(0);
        Ok(count > 0)
    }

    pub async fn get_layer_path(&self, hash: &str) -> Result<Option<String>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT storage_path FROM cache_layers WHERE layer_hash = $1",
                &[&hash],
            )
            .await?;

        if let Some(row) = rows.first() {
            Ok(Some(row.get(0)))
        } else {
            Ok(None)
        }
    }

    pub async fn get(&self, hash: &str) -> Result<Option<CacheEntry>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"
                SELECT hash, artifact_path, size, created_at, last_used, hit_count, is_layered
                FROM cache_entries WHERE hash = $1
                "#,
                &[&hash],
            )
            .await?;

        if let Some(row) = rows.first() {
            Ok(Some(CacheEntry {
                hash: row.get(0),
                artifact_path: row.get(1),
                size: row.get(2),
                created_at: row.get(3),
                last_used: row.get(4),
                hit_count: row.get(5),
                is_layered: row.get(6),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn cleanup_old_entries(&self, days: u32) -> Result<i64> {
        let client = self.pool.get().await?;
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);

        let deleted = client
            .execute("DELETE FROM cache_entries WHERE last_used < $1", &[&cutoff])
            .await?;

        Ok(deleted as i64)
    }

    pub async fn get_stats(&self) -> Result<DatabaseStats> {
        let client = self.pool.get().await?;

        let total_entries: i64 = client
            .query_one("SELECT COUNT(*) FROM cache_entries", &[])
            .await?
            .get(0);

        let total_size: i64 = client
            .query_one("SELECT COALESCE(SUM(size), 0) FROM cache_entries", &[])
            .await?
            .get(0);

        let total_layers: i64 = client
            .query_one("SELECT COUNT(*) FROM cache_layers", &[])
            .await?
            .get(0);

        let layer_size: i64 = client
            .query_one("SELECT COALESCE(SUM(size), 0) FROM cache_layers", &[])
            .await?
            .get(0);

        Ok(DatabaseStats {
            total_entries,
            total_size,
            total_layers,
            layer_size,
            total_size_all: total_size + layer_size,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStats {
    pub total_entries: i64,
    pub total_size: i64,
    pub total_layers: i64,
    pub layer_size: i64,
    pub total_size_all: i64,
}

/// Read replica configuration for load distribution
#[derive(Debug, Clone)]
pub struct ReadReplicaConfig {
    pub replicas: Vec<PostgresConfig>,
}

/// Metadata store with read replicas for scaling reads
pub struct ReplicatedMetadataStore {
    _writer: PostgresMetadataStore,
    _readers: Vec<PostgresMetadataStore>,
    _next_reader: std::sync::atomic::AtomicUsize,
}

impl ReplicatedMetadataStore {
    pub async fn new(
        writer_config: PostgresConfig,
        replica_configs: Vec<PostgresConfig>,
    ) -> Result<Self> {
        let writer = PostgresMetadataStore::new(writer_config).await?;

        let mut readers = Vec::new();
        for config in replica_configs {
            readers.push(PostgresMetadataStore::new(config).await?);
        }

        Ok(Self {
            _writer: writer,
            _readers: readers,
            _next_reader: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn _get_reader(&self) -> &PostgresMetadataStore {
        if self._readers.is_empty() {
            &self._writer
        } else {
            let idx = self
                ._next_reader
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                % self._readers.len();
            &self._readers[idx]
        }
    }
}

#[async_trait]
impl crate::server::metadata::MetadataStoreTrait for PostgresMetadataStore {
    async fn insert(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        self.insert(hash, path, size as i64).await
    }

    async fn insert_layered_node(
        &self,
        hash: &str,
        size: u64,
        layer_hashes: &[String],
    ) -> Result<()> {
        self.insert_layered_node(hash, size as i64, layer_hashes)
            .await
    }

    async fn insert_layer(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        self.insert_layer(hash, path, size as i64).await
    }

    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        self.get_node_layers(hash).await
    }

    async fn layer_exists(&self, hash: &str) -> Result<bool> {
        self.layer_exists(hash).await
    }

    async fn get_layer_path(&self, hash: &str) -> Result<Option<String>> {
        self.get_layer_path(hash).await
    }

    async fn get(&self, hash: &str) -> Result<Option<crate::server::metadata::CacheEntry>> {
        match self.get(hash).await? {
            Some(entry) => Ok(Some(crate::server::metadata::CacheEntry {
                hash: entry.hash,
                artifact_path: entry.artifact_path,
                size: entry.size as u64,
                created_at: entry.created_at.to_rfc3339(),
                last_used: entry.last_used.to_rfc3339(),
                hit_count: entry.hit_count as u32,
            })),
            None => Ok(None),
        }
    }

    async fn cleanup_old_entries(&self, days: u32) -> Result<i64> {
        self.cleanup_old_entries(days).await
    }
}
