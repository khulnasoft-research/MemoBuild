use super::ArtifactStorage;
use anyhow::Result;
use futures::{Stream, TryStreamExt};
use google_cloud_storage::client::{Client, ClientConfig};
use google_cloud_storage::http::objects::delete::DeleteObjectRequest;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use google_cloud_storage::http::objects::upload::{Media, UploadObjectRequest, UploadType};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// Google Cloud Storage backed artifact storage.
///
/// Uses the `google-cloud-storage` crate with Application Default Credentials.
/// Set `GOOGLE_APPLICATION_CREDENTIALS` or run on GCE/GKE with a service account.
#[derive(Clone)]
pub struct GcsStorage {
    client: Arc<OnceCell<Client>>,
    bucket: String,
    prefix: String,
}

impl GcsStorage {
    pub fn new_sync(bucket: String, prefix: String) -> Self {
        Self {
            client: Arc::new(OnceCell::new()),
            bucket,
            prefix,
        }
    }

    async fn get_client(&self) -> Result<&Client> {
        self.client
            .get_or_try_init(|| async {
                let config = ClientConfig::default()
                    .with_auth()
                    .await
                    .map_err(|e| anyhow::anyhow!("GCS auth failed: {}", e))?;
                Ok(Client::new(config))
            })
            .await
            .map_err(|e: anyhow::Error| e)
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
        let name = self.object_name(hash);
        let bucket = self.bucket.clone();
        let data = data.to_vec();
        let rt = tokio::runtime::Handle::current();

        let client = rt.block_on(self.get_client())?;

        let media = Media::new(name.clone());
        let upload_type = UploadType::Simple(media);
        let req = UploadObjectRequest {
            bucket,
            ..Default::default()
        };

        rt.block_on(client.upload_object(&req, data, &upload_type))
            .map_err(|e| anyhow::anyhow!("GCS upload failed: {}", e))?;

        Ok(format!("gs://{}/{}", self.bucket, name))
    }

    fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let name = self.object_name(hash);
        let bucket = self.bucket.clone();
        let rt = tokio::runtime::Handle::current();

        let client = rt.block_on(self.get_client())?;

        let req = GetObjectRequest {
            bucket,
            object: name,
            ..Default::default()
        };

        let result = rt.block_on(client.download_object(&req, &Range::default()));
        match result {
            Ok(data) => Ok(Some(data)),
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("NotFound") || msg.contains("not found") {
                    return Ok(None);
                }
                Err(anyhow::anyhow!("GCS get failed: {}", e))
            }
        }
    }

    fn stream_get<'a>(
        &'a self,
        hash: &'a str,
    ) -> Result<Option<Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send + 'a>>>> {
        let name = self.object_name(hash);
        let bucket = self.bucket.clone();
        let rt = tokio::runtime::Handle::current();

        let client = rt.block_on(self.get_client())?;

        let req = GetObjectRequest {
            bucket: bucket.clone(),
            object: name.clone(),
            ..Default::default()
        };

        let stream_result = rt.block_on(client.download_streamed_object(&req, &Range::default()));
        match stream_result {
            Ok(stream) => {
                let mapped = stream
                    .map_ok(|bytes| bytes.to_vec())
                    .map_err(|e| anyhow::anyhow!("GCS stream error: {}", e));
                Ok(Some(Box::pin(mapped)))
            }
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("NotFound") || msg.contains("not found") {
                    return Ok(None);
                }
                Err(anyhow::anyhow!("GCS stream_get failed: {}", e))
            }
        }
    }

    fn exists(&self, hash: &str) -> Result<bool> {
        let name = self.object_name(hash);
        let bucket = self.bucket.clone();
        let rt = tokio::runtime::Handle::current();

        let client = rt.block_on(self.get_client())?;

        let req = GetObjectRequest {
            bucket,
            object: name,
            ..Default::default()
        };

        let result = rt.block_on(client.download_object(&req, &Range(Some(0), Some(0))));
        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("NotFound") || msg.contains("not found") {
                    Ok(false)
                } else {
                    Err(anyhow::anyhow!("GCS exists check failed: {}", e))
                }
            }
        }
    }

    fn delete(&self, hash: &str) -> Result<()> {
        let name = self.object_name(hash);
        let bucket = self.bucket.clone();
        let rt = tokio::runtime::Handle::current();

        let client = rt.block_on(self.get_client())?;

        let req = DeleteObjectRequest {
            bucket,
            object: name,
            ..Default::default()
        };

        rt.block_on(client.delete_object(&req))
            .map_err(|e| anyhow::anyhow!("GCS delete failed: {}", e))?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::storage::ArtifactStorageAsync for GcsStorage {
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
