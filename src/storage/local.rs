use super::ArtifactStorage;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct LocalStorage {
    base_dir: PathBuf,
}

impl LocalStorage {
    pub fn new(base_dir: &Path) -> Result<Self> {
        let blobs_dir = base_dir.join("blobs").join("sha256");
        fs::create_dir_all(&blobs_dir)?;
        Ok(Self {
            base_dir: blobs_dir,
        })
    }

    fn get_sharded_path(&self, hash: &str) -> PathBuf {
        if hash.len() < 4 {
            return self.base_dir.join(hash);
        }
        let shard1 = &hash[0..2];
        let shard2 = &hash[2..4];
        self.base_dir.join(shard1).join(shard2).join(hash)
    }
}

impl ArtifactStorage for LocalStorage {
    fn put(&self, hash: &str, data: &[u8]) -> Result<String> {
        let path = self.get_sharded_path(hash);

        if path.exists() {
            return Ok(path.to_string_lossy().to_string());
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&path)
            .with_context(|| format!("Failed to create artifact file at {}", path.display()))?;
        file.write_all(data)?;

        Ok(path.to_string_lossy().to_string())
    }

    fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let path = self.get_sharded_path(hash);
        if path.exists() {
            let data = fs::read(&path)?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    fn exists(&self, hash: &str) -> Result<bool> {
        Ok(self.get_sharded_path(hash).exists())
    }

    fn delete(&self, hash: &str) -> Result<()> {
        let path = self.get_sharded_path(hash);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_local_storage() {
        let dir = tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();

        let hash = "abcdef123456";
        let data = b"test-data";

        storage.put(hash, data).unwrap();
        assert!(storage.exists(hash).unwrap());

        let retrieved = storage.get(hash).unwrap().unwrap();
        assert_eq!(retrieved, data);

        let path = storage.get_sharded_path(hash);
        assert!(path.to_string_lossy().contains("ab/cd/abcdef"));
    }
}
