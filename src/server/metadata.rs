use anyhow::Result;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

#[async_trait]
pub trait MetadataStoreTrait: Send + Sync {
    async fn insert(&self, hash: &str, path: &str, size: u64) -> Result<()>;
    async fn insert_layered_node(
        &self,
        hash: &str,
        size: u64,
        layer_hashes: &[String],
    ) -> Result<()>;
    async fn insert_layer(&self, hash: &str, path: &str, size: u64) -> Result<()>;
    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>>;
    async fn layer_exists(&self, hash: &str) -> Result<bool>;
    async fn get_layer_path(&self, hash: &str) -> Result<Option<String>>;
    async fn get(&self, hash: &str) -> Result<Option<CacheEntry>>;
    async fn cleanup_old_entries(&self, days: u32) -> Result<i64>;
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub hash: String,
    pub artifact_path: String,
    pub size: u64,
    pub created_at: String,
    pub last_used: String,
    pub hit_count: u32,
}

pub struct MetadataStore {
    conn: Mutex<Connection>,
}

impl MetadataStore {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                hash TEXT PRIMARY KEY,
                artifact_path TEXT,
                size BIGINT,
                created_at TIMESTAMP,
                last_used TIMESTAMP,
                hit_count INT,
                is_layered BOOLEAN DEFAULT FALSE
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_layers (
                layer_hash TEXT PRIMARY KEY,
                size BIGINT,
                storage_path TEXT,
                created_at TIMESTAMP,
                last_used TIMESTAMP,
                ref_count INT DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS node_to_layers (
                node_hash TEXT,
                layer_hash TEXT,
                position INT,
                PRIMARY KEY(node_hash, position),
                FOREIGN KEY(node_hash) REFERENCES cache_entries(hash),
                FOREIGN KEY(layer_hash) REFERENCES cache_layers(layer_hash)
            )",
            [],
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cache_entries (hash, artifact_path, size, created_at, last_used, hit_count, is_layered)
             VALUES (?1, ?2, ?3, ?4, ?4, 0, FALSE)
             ON CONFLICT(hash) DO UPDATE SET
                last_used = ?4,
                hit_count = hit_count + 1",
            params![hash, path, size, now],
        )?;
        Ok(())
    }

    pub fn insert_layered_node(
        &self,
        hash: &str,
        size: u64,
        layer_hashes: &[String],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = chrono::Utc::now().to_rfc3339();

        tx.execute(
            "INSERT INTO cache_entries (hash, artifact_path, size, created_at, last_used, hit_count, is_layered)
             VALUES (?1, '', ?2, ?3, ?3, 0, TRUE)
             ON CONFLICT(hash) DO UPDATE SET
                last_used = ?3,
                hit_count = hit_count + 1",
            params![hash, size, now],
        )?;

        // Remove old mappings
        tx.execute(
            "DELETE FROM node_to_layers WHERE node_hash = ?1",
            params![hash],
        )?;

        for (pos, layer_hash) in layer_hashes.iter().enumerate() {
            tx.execute(
                "INSERT INTO node_to_layers (node_hash, layer_hash, position) VALUES (?1, ?2, ?3)",
                params![hash, layer_hash, pos as i32],
            )?;

            // Increment ref count for existing layers
            tx.execute(
                "UPDATE cache_layers SET ref_count = ref_count + 1 WHERE layer_hash = ?1",
                params![layer_hash],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn insert_layer(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cache_layers (layer_hash, storage_path, size, created_at, last_used)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(layer_hash) DO UPDATE SET
                last_used = ?4",
            params![hash, path, size, now],
        )?;
        Ok(())
    }

    pub fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT layer_hash FROM node_to_layers WHERE node_hash = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![hash], |row| row.get(0))?;

        let mut layers = Vec::new();
        for layer in rows {
            layers.push(layer?);
        }

        if layers.is_empty() {
            // Check if node exists at all
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cache_entries WHERE hash = ?1",
                params![hash],
                |row| row.get(0),
            )?;
            if count == 0 {
                return Ok(None);
            }
        }

        Ok(Some(layers))
    }

    pub fn layer_exists(&self, hash: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cache_layers WHERE layer_hash = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_layer_path(&self, hash: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT storage_path FROM cache_layers WHERE layer_hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn get(&self, hash: &str) -> Result<Option<CacheEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT hash, artifact_path, size, created_at, last_used, hit_count FROM cache_entries WHERE hash = ?1",
        )?;
        let mut rows = stmt.query(params![hash])?;

        if let Some(row) = rows.next()? {
            Ok(Some(CacheEntry {
                hash: row.get(0)?,
                artifact_path: row.get(1)?,
                size: row.get(2)?,
                created_at: row.get(3)?,
                last_used: row.get(4)?,
                hit_count: row.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn touch(&self, hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE cache_entries SET last_used = ?1, hit_count = hit_count + 1 WHERE hash = ?2",
            params![now, hash],
        )?;
        Ok(())
    }

    pub fn exists(&self, hash: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cache_entries WHERE hash = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn delete(&self, hash: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        // Decrement ref counts for layers
        tx.execute(
            "UPDATE cache_layers SET ref_count = ref_count - 1 
             WHERE layer_hash IN (SELECT layer_hash FROM node_to_layers WHERE node_hash = ?1)",
            params![hash],
        )?;

        // Delete mappings
        tx.execute(
            "DELETE FROM node_to_layers WHERE node_hash = ?1",
            params![hash],
        )?;

        // Delete node
        tx.execute("DELETE FROM cache_entries WHERE hash = ?1", params![hash])?;

        tx.commit()?;
        Ok(())
    }

    pub fn get_unused_layers(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT layer_hash, storage_path FROM cache_layers WHERE ref_count <= 0")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        let mut layers = Vec::new();
        for layer in rows {
            layers.push(layer?);
        }
        Ok(layers)
    }

    pub fn delete_layer_metadata(&self, hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM cache_layers WHERE layer_hash = ?1",
            params![hash],
        )?;
        Ok(())
    }

    pub fn get_layer_stats(&self) -> Result<LayerStats> {
        let conn = self.conn.lock().unwrap();
        let total_layers: i64 =
            conn.query_row("SELECT COUNT(*) FROM cache_layers", [], |row| row.get(0))?;
        let total_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM cache_layers",
            [],
            |row| row.get(0),
        )?;
        let deduplicated_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM cache_layers WHERE ref_count > 1",
            [],
            |row| row.get(0),
        )?;

        Ok(LayerStats {
            total_layers: total_layers as u32,
            total_size: total_size as u64,
            deduplicated_size: deduplicated_size as u64,
        })
    }

    pub fn get_old_entries(&self, days: u32) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT hash FROM cache_entries WHERE last_used < datetime('now', '-' || ?1 || ' days')"
        )?;
        let rows = stmt.query_map(params![days], |row| row.get(0))?;

        let mut hashes = Vec::new();
        for hash in rows {
            hashes.push(hash?);
        }
        Ok(hashes)
    }

    pub fn record_build(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS build_analytics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT DEFAULT CURRENT_TIMESTAMP,
                dirty_nodes INTEGER,
                cached_nodes INTEGER,
                duration_ms INTEGER
            )",
            [],
        )?;

        conn.execute(
            "INSERT INTO build_analytics (dirty_nodes, cached_nodes, duration_ms) VALUES (?1, ?2, ?3)",
            params![dirty, cached, duration_ms],
        )?;

        Ok(())
    }

    pub fn get_analytics(&self, limit: u32) -> Result<Vec<BuildRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, dirty_nodes, cached_nodes, duration_ms 
             FROM build_analytics 
             ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(BuildRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                dirty_nodes: row.get(2)?,
                cached_nodes: row.get(3)?,
                duration_ms: row.get(4)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

#[async_trait]
impl MetadataStoreTrait for MetadataStore {
    async fn insert(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        MetadataStore::insert(self, hash, path, size)
    }

    async fn insert_layered_node(
        &self,
        hash: &str,
        size: u64,
        layer_hashes: &[String],
    ) -> Result<()> {
        MetadataStore::insert_layered_node(self, hash, size, layer_hashes)
    }

    async fn insert_layer(&self, hash: &str, path: &str, size: u64) -> Result<()> {
        MetadataStore::insert_layer(self, hash, path, size)
    }

    async fn get_node_layers(&self, hash: &str) -> Result<Option<Vec<String>>> {
        MetadataStore::get_node_layers(self, hash)
    }

    async fn layer_exists(&self, hash: &str) -> Result<bool> {
        MetadataStore::layer_exists(self, hash)
    }

    async fn get_layer_path(&self, hash: &str) -> Result<Option<String>> {
        MetadataStore::get_layer_path(self, hash)
    }

    async fn get(&self, hash: &str) -> Result<Option<CacheEntry>> {
        MetadataStore::get(self, hash)
    }

    async fn cleanup_old_entries(&self, days: u32) -> Result<i64> {
        let entries = MetadataStore::get_old_entries(self, days)?;
        let count = entries.len() as i64;
        for hash in entries {
            MetadataStore::delete(self, &hash)?;
        }
        Ok(count)
    }
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BuildRecord {
    pub id: i32,
    pub timestamp: String,
    pub dirty_nodes: u32,
    pub cached_nodes: u32,
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LayerStats {
    pub total_layers: u32,
    pub total_size: u64,
    pub deduplicated_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_metadata_store() {
        let db_file = NamedTempFile::new().unwrap();
        let store = MetadataStore::new(db_file.path()).unwrap();

        let hash = "test-hash";
        let path = "some/path";
        let size = 1024;

        store.insert(hash, path, size).unwrap();
        assert!(store.exists(hash).unwrap());

        let entry = store.get(hash).unwrap().unwrap();
        assert_eq!(entry.hash, hash);
        assert_eq!(entry.artifact_path, path);
        assert_eq!(entry.size, size);

        store.touch(hash).unwrap();
        let updated_entry = store.get(hash).unwrap().unwrap();
        assert_eq!(updated_entry.hit_count, 1);
    }
}
