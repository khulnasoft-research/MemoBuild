pub mod gcs;
pub mod local;
pub mod s3;

use anyhow::Result;

pub trait ArtifactStorage: Send + Sync {
    fn put(&self, hash: &str, data: &[u8]) -> Result<String>;
    fn get(&self, hash: &str) -> Result<Option<Vec<u8>>>;
    fn exists(&self, hash: &str) -> Result<bool>;
    fn delete(&self, hash: &str) -> Result<()>;
}

pub use gcs::GcsStorage;
pub use local::LocalStorage;
pub use s3::S3Storage;

/// Backend selection for artifact storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Local,
    S3,
    Gcs,
}

impl StorageBackend {
    pub fn from_env() -> Self {
        match std::env::var("MEMOBUILD_STORAGE_BACKEND")
            .unwrap_or_else(|_| "local".to_string())
            .to_lowercase()
            .as_str()
        {
            "s3" => StorageBackend::S3,
            "gcs" => StorageBackend::Gcs,
            _ => StorageBackend::Local,
        }
    }
}

/// Factory: build a concrete `ArtifactStorage` from environment variables.
///
/// * `MEMOBUILD_STORAGE_BACKEND` — `local` (default), `s3`, `gcs`
/// * `MEMOBUILD_STORAGE_BUCKET` — bucket name (S3/GCS)
/// * `MEMOBUILD_STORAGE_ENDPOINT` — custom endpoint (MinIO, LocalStack)
/// * `MEMOBUILD_STORAGE_REGION` — AWS region (default `us-east-1`)
/// * `MEMOBUILD_STORAGE_PREFIX` — key prefix inside the bucket
pub fn storage_from_env(base_dir: &std::path::Path) -> Result<Box<dyn ArtifactStorage>> {
    let backend = StorageBackend::from_env();
    match backend {
        StorageBackend::Local => Ok(Box::new(LocalStorage::new(base_dir)?)),
        StorageBackend::S3 => {
            let bucket = std::env::var("MEMOBUILD_STORAGE_BUCKET")
                .expect("MEMOBUILD_STORAGE_BUCKET required for s3 backend");
            let endpoint = std::env::var("MEMOBUILD_STORAGE_ENDPOINT").ok();
            let region = std::env::var("MEMOBUILD_STORAGE_REGION")
                .unwrap_or_else(|_| "us-east-1".to_string());
            let prefix = std::env::var("MEMOBUILD_STORAGE_PREFIX").unwrap_or_default();
            // S3Storage::new is async, but we construct a blocking wrapper here.
            // The server calls this at startup inside a tokio runtime.
            Ok(Box::new(S3Storage::new_sync(bucket, endpoint, region, prefix)))
        }
        StorageBackend::Gcs => {
            let bucket = std::env::var("MEMOBUILD_STORAGE_BUCKET")
                .expect("MEMOBUILD_STORAGE_BUCKET required for gcs backend");
            let prefix = std::env::var("MEMOBUILD_STORAGE_PREFIX").unwrap_or_default();
            Ok(Box::new(GcsStorage::new_sync(bucket, prefix)))
        }
    }
}
