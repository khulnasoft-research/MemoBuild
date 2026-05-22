use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub cache_key: String,
    pub created_at: i64,
    pub artifact_path: PathBuf,
    pub size: u64,
}

use std::sync::{Arc, RwLock};

pub struct LocalCache {
    cache_dir: PathBuf,
    store: Arc<RwLock<HashMap<String, CacheEntry>>>,
    index_path: PathBuf,
}

impl LocalCache {
    pub fn new() -> Result<Self> {
        let cache_dir = Self::get_cache_dir()?;
        fs::create_dir_all(&cache_dir)?;

        let index_path = cache_dir.join("index.json");
        let store = Self::load_index(&index_path)?;

        Ok(Self {
            cache_dir,
            store: Arc::new(RwLock::new(store)),
            index_path,
        })
    }

    fn get_cache_dir() -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("MEMOBUILD_CACHE_DIR") {
            return Ok(PathBuf::from(dir));
        }
        let home = std::env::var("HOME").context("HOME environment variable not set")?;
        Ok(PathBuf::from(home).join(".memobuild").join("cache"))
    }

    fn load_index(path: &Path) -> Result<HashMap<String, CacheEntry>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let content = fs::read_to_string(path)?;
        let store: HashMap<String, CacheEntry> = serde_json::from_str(&content).unwrap_or_default();

        Ok(store)
    }

    fn save_index(&self) -> Result<()> {
        let store = self
            .store
            .read()
            .map_err(|_| anyhow::anyhow!("Poisoned lock"))?;
        let content = serde_json::to_string_pretty(&*store)?;
        fs::write(&self.index_path, content)?;
        Ok(())
    }

    pub fn get_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let store = self
            .store
            .read()
            .map_err(|_| anyhow::anyhow!("Poisoned lock"))?;
        if let Some(entry) = store.get(key) {
            let path = self.cache_dir.join(&entry.artifact_path);
            if path.exists() {
                return Ok(Some(fs::read(path)?));
            }
        }
        Ok(None)
    }

    pub fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let artifact_filename = format!("{}.bin", key);
        let artifact_path = PathBuf::from(&artifact_filename);
        let full_path = self.cache_dir.join(&artifact_path);

        fs::write(&full_path, data)?;

        let entry = CacheEntry {
            cache_key: key.to_string(),
            created_at: chrono::Utc::now().timestamp(),
            artifact_path,
            size: data.len() as u64,
        };

        {
            let mut store = self
                .store
                .write()
                .map_err(|_| anyhow::anyhow!("Poisoned lock"))?;
            store.insert(key.to_string(), entry);
        }

        self.save_index()?;

        Ok(())
    }

    pub fn exists(&self, key: &str) -> bool {
        let store = self.store.read().ok();
        store.map(|s| s.contains_key(key)).unwrap_or(false)
    }
}