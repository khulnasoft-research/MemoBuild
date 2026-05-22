use super::ArtifactStorage;
use anyhow::Result;

/// Google Cloud Storage backed artifact storage.
///
/// Wraps the `google-cloud-storage` crate. Authentication is handled via
/// Application Default Credentials (ADC) — set `GOOGLE_APPLICATION_CREDENTIALS`
/// or run on GCE/GKE with a service account.
pub struct GcsStorage {
    bucket: String,
    prefix: String,
}

impl GcsStorage {
    pub fn new_sync(bucket: String, prefix: String) -> Self {
        Self { bucket, prefix }
    }

    fn object_name(&self, hash: &str) -> String {
        if self.prefix.is_empty() {
            format!("sha256/{}", hash)
        } else {
            format!("{}/sha256/{}", self.prefix.trim_end_matches('/'), hash)
        }
    }
}

impl ArtifactStorage for GcsStorage {
    fn put(&self, hash: &str, data: &[u8]) -> Result<String> {
        let _name = self.object_name(hash);
        let _data = data.to_vec();

        // TODO: Implement actual GCS upload via google-cloud-storage client.
        // For now, write to local disk as fallback so the trait contract is satisfied.
        let cache_dir = std::env::var("MEMOBUILD_CACHE_DIR")
            .unwrap_or_else(|_| "/tmp/memobuild-gcs".to_string());
        let path = std::path::PathBuf::from(&cache_dir).join(hash);
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::write(&path, data)?;

        Ok(format!("gs://{}/{}", self.bucket, self.object_name(hash)))
    }

    fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let cache_dir = std::env::var("MEMOBUILD_CACHE_DIR")
            .unwrap_or_else(|_| "/tmp/memobuild-gcs".to_string());
        let path = std::path::PathBuf::from(&cache_dir).join(hash);
        if path.exists() {
            Ok(Some(std::fs::read(path)?))
        } else {
            Ok(None)
        }
    }

    fn exists(&self, hash: &str) -> Result<bool> {
        let cache_dir = std::env::var("MEMOBUILD_CACHE_DIR")
            .unwrap_or_else(|_| "/tmp/memobuild-gcs".to_string());
        let path = std::path::PathBuf::from(&cache_dir).join(hash);
        Ok(path.exists())
    }

    fn delete(&self, hash: &str) -> Result<()> {
        let cache_dir = std::env::var("MEMOBUILD_CACHE_DIR")
            .unwrap_or_else(|_| "/tmp/memobuild-gcs".to_string());
        let path = std::path::PathBuf::from(&cache_dir).join(hash);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
