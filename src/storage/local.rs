use super::ArtifactStorage;
use anyhow::{Context, Result};
use futures::Stream;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[derive(Clone)]
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

    fn stream_get<'a>(
        &'a self,
        hash: &'a str,
    ) -> Result<Option<Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send + 'a>>>> {
        let path = self.get_sharded_path(hash);
        if !path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(&path)?;
        let stream = file_stream(file);
        Ok(Some(Box::pin(stream)))
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

#[async_trait::async_trait]
impl crate::storage::ArtifactStorageAsync for LocalStorage {
    async fn put_async(&self, hash: &str, data: &[u8]) -> Result<String> {
        let s = self.clone();
        let hash = hash.to_string();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || s.put(&hash, &data))
            .await
            .map_err(|e| anyhow::anyhow!("join error: {}", e))?
    }

    async fn get_async(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let s = self.clone();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || s.get(&hash))
            .await
            .map_err(|e| anyhow::anyhow!("join error: {}", e))?
    }

    async fn stream_get_async<'a>(
        &'a self,
        hash: &'a str,
    ) -> Result<Option<Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send + 'a>>>> {
        let s = self.clone();
        let hash = hash.to_string();
        let result = tokio::task::spawn_blocking(move || s.get(&hash))
            .await
            .map_err(|e| anyhow::anyhow!("join error: {}", e))??;

        Ok(result.map(|data| {
            Box::pin(futures::stream::once(async move { Ok(data) }))
                as Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send + 'a>>
        }))
    }

    async fn exists_async(&self, hash: &str) -> Result<bool> {
        let s = self.clone();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || s.exists(&hash))
            .await
            .map_err(|e| anyhow::anyhow!("join error: {}", e))?
    }

    async fn delete_async(&self, hash: &str) -> Result<()> {
        let s = self.clone();
        let hash = hash.to_string();
        tokio::task::spawn_blocking(move || s.delete(&hash))
            .await
            .map_err(|e| anyhow::anyhow!("join error: {}", e))?
    }
}

fn file_stream(file: fs::File) -> impl Stream<Item = Result<Vec<u8>>> {
    const CHUNK_SIZE: usize = 64 * 1024;
    futures::stream::unfold((file, false), move |(mut f, eof)| async move {
        if eof {
            return None;
        }
        let mut buf = vec![0u8; CHUNK_SIZE];
        match f.read(&mut buf) {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(buf), (f, false)))
            }
            Err(e) => Some((Err(anyhow::Error::from(e)), (f, true))),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ArtifactStorageAsync;
    use futures::TryStreamExt;
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

    #[tokio::test]
    async fn test_local_storage_async_trait() {
        let dir = tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();

        let hash = "abcdef123456";
        let data = b"async-test-data".to_vec();

        let path = crate::storage::ArtifactStorageAsync::put_async(&storage, hash, &data)
            .await
            .unwrap();
        assert!(!path.is_empty());
        assert!(crate::storage::ArtifactStorageAsync::exists_async(&storage, hash)
            .await
            .unwrap());

        let retrieved = crate::storage::ArtifactStorageAsync::get_async(&storage, hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved, data);

        let stream = crate::storage::ArtifactStorageAsync::stream_get_async(&storage, hash)
            .await
            .unwrap()
            .unwrap();
        let chunks = stream.try_collect::<Vec<_>>().await.unwrap();
        assert_eq!(chunks.concat(), data);

        crate::storage::ArtifactStorageAsync::delete_async(&storage, hash)
            .await
            .unwrap();
        assert!(!crate::storage::ArtifactStorageAsync::exists_async(&storage, hash)
            .await
            .unwrap());
    }
}
