use super::ArtifactStorage;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// S3-backed artifact storage.
///
/// Uses the AWS SDK for S3. Compatible with MinIO, LocalStack, and any
/// S3-compatible endpoint via `MEMOBUILD_STORAGE_ENDPOINT`.
pub struct S3Storage {
    client: Arc<OnceCell<aws_sdk_s3::Client>>,
    bucket: String,
    prefix: String,
    region: String,
    endpoint: Option<String>,
}

impl S3Storage {
    pub fn new_sync(
        bucket: String,
        endpoint: Option<String>,
        region: String,
        prefix: String,
    ) -> Self {
        Self {
            client: Arc::new(OnceCell::new()),
            bucket,
            prefix,
            region,
            endpoint,
        }
    }

    pub async fn new(
        bucket: String,
        endpoint: Option<String>,
        region: String,
        prefix: String,
    ) -> Self {
        let mut s3_config_builder = aws_sdk_s3::config::Builder::new()
            .region(aws_sdk_s3::config::Region::new(region.clone()));

        if let Some(ep) = endpoint.clone() {
            s3_config_builder = s3_config_builder.endpoint_url(ep);
        }

        let s3_config = s3_config_builder.build();
        let client = aws_sdk_s3::Client::from_conf(s3_config);

        Self {
            client: Arc::new(OnceCell::new_with(Some(client))),
            bucket,
            prefix,
            region,
            endpoint,
        }
    }

    fn key(&self, hash: &str) -> String {
        if self.prefix.is_empty() {
            format!("sha256/{}", hash)
        } else {
            format!("{}/sha256/{}", self.prefix.trim_end_matches('/'), hash)
        }
    }

    async fn get_client(&self) -> &aws_sdk_s3::Client {
        self.client
            .get_or_init(|| async {
                let mut s3_config_builder = aws_sdk_s3::config::Builder::new()
                    .region(aws_sdk_s3::config::Region::new(self.region.clone()));

                if let Some(ref ep) = self.endpoint {
                    s3_config_builder = s3_config_builder.endpoint_url(ep.clone());
                }

                let s3_config = s3_config_builder.build();
                aws_sdk_s3::Client::from_conf(s3_config)
            })
            .await
    }
}

impl ArtifactStorage for S3Storage {
    fn put(&self, hash: &str, data: &[u8]) -> Result<String> {
        let key = self.key(hash);
        let bucket = self.bucket.clone();
        let data = data.to_vec();

        let rt = tokio::runtime::Handle::current();
        let client = rt.block_on(self.get_client());

        rt.block_on(
            client
                .put_object()
                .bucket(&bucket)
                .key(&key)
                .body(data.into())
                .send(),
        )
        .map_err(|e| anyhow::anyhow!("S3 put failed: {}", e))?;

        Ok(format!("s3://{}/{}", bucket, key))
    }

    fn get(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let key = self.key(hash);
        let bucket = self.bucket.clone();

        let rt = tokio::runtime::Handle::current();
        let client = rt.block_on(self.get_client());

        let resp = match rt.block_on(
            client
                .get_object()
                .bucket(&bucket)
                .key(&key)
                .send(),
        ) {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("NoSuchKey") || msg.contains("NotFound") {
                    return Ok(None);
                }
                return Err(anyhow::anyhow!("S3 get failed: {}", e));
            }
        };

        let body = rt
            .block_on(resp.body.collect())
            .map_err(|e| anyhow::anyhow!("S3 read body failed: {}", e))?
            .into_bytes()
            .to_vec();

        Ok(Some(body))
    }

    fn exists(&self, hash: &str) -> Result<bool> {
        let key = self.key(hash);
        let bucket = self.bucket.clone();

        let rt = tokio::runtime::Handle::current();
        let client = rt.block_on(self.get_client());

        match rt.block_on(
            client
                .head_object()
                .bucket(&bucket)
                .key(&key)
                .send(),
        ) {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("NotFound") || msg.contains("NoSuchKey") {
                    return Ok(false);
                }
                Err(anyhow::anyhow!("S3 head failed: {}", e))
            }
        }
    }

    fn delete(&self, hash: &str) -> Result<()> {
        let key = self.key(hash);
        let bucket = self.bucket.clone();

        let rt = tokio::runtime::Handle::current();
        let client = rt.block_on(self.get_client());

        rt.block_on(
            client
                .delete_object()
                .bucket(&bucket)
                .key(&key)
                .send(),
        )
        .map_err(|e| anyhow::anyhow!("S3 delete failed: {}", e))?;

        Ok(())
    }
}
