use crate::cache::remote::RemoteCache;
use crate::cache::local::LocalCache;
use anyhow::Result;
use std::sync::Arc;

pub struct HybridCache {
    pub local: LocalCache,
    pub remote: Option<Arc<dyn RemoteCache>>,
}

impl HybridCache {
    pub fn new(remote: Option<Arc<dyn RemoteCache>>) -> Result<Self> {
        Ok(Self {
            local: LocalCache::new()?,
            remote,
        })
    }

    pub fn new_with_box(remote: Option<Arc<dyn RemoteCache>>) -> Result<Self> {
        Self::new(remote)
    }

    pub async fn get_artifact(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // 1. Try local
        if let Some(data) = self.local.get_data(key)? {
            return Ok(Some(data));
        }

        // 2. Try remote (Layered protocol)
        if let Some(ref remote) = self.remote {
            if let Some(layer_hashes) = remote.get_node_layers(key).await? {
                println!(
                    "   📦 Reconstructing artifact from {} layers...",
                    layer_hashes.len()
                );
                let mut layers_data = Vec::with_capacity(layer_hashes.len());
                for hash in layer_hashes {
                    if let Some(layer) = remote.get_layer(&hash).await? {
                        layers_data.push(layer);
                    } else {
                        anyhow::bail!(
                            "Cache integrity failure: layer {} missing for node {}",
                            hash,
                            key
                        );
                    }
                }
                let data = crate::cache::utils::merge_artifact(layers_data);
                self.local.put(key, &data)?;
                return Ok(Some(data));
            }

            // Fallback for non-layered artifacts
            if let Some(data) = remote.get(key).await? {
                // Populate local cache
                self.local.put(key, &data)?;
                return Ok(Some(data));
            }
        }

        Ok(None)
    }

    pub async fn put_artifact(&self, key: &str, data: &[u8]) -> Result<()> {
        // 1. Put local
        self.local.put(key, data)?;

        // 2. Put remote (Layered protocol)
        if let Some(ref remote) = self.remote {
            let layers = crate::cache::utils::split_artifact(data);
            let mut layer_hashes = Vec::new();

            for layer in layers {
                layer_hashes.push(layer.hash.clone());
                if !remote.has_layer(&layer.hash).await? {
                    remote.put_layer(&layer.hash, &layer.data).await?;
                }
            }

            remote
                .register_node_layers(key, &layer_hashes, data.len() as u64)
                .await?;
        }

        Ok(())
    }

    pub async fn report_analytics(&self, dirty: u32, cached: u32, duration_ms: u64) -> Result<()> {
        if let Some(ref remote) = self.remote {
            remote.report_analytics(dirty, cached, duration_ms).await?;
        }
        Ok(())
    }

    /// Smart Prefetching: Start downloading artifacts in the background
    pub fn prefetch_artifacts(self: Arc<Self>, hashes: Vec<String>) {
        for hash in hashes {
            // Check local existence first (lightweight)
            if self.local.exists(&hash) {
                continue;
            }

            let cache_clone = self.clone();
            let hash_clone = hash.clone();

            // Spawn background task to fetch from remote
            tokio::task::spawn(async move {
                if let Some(ref remote) = cache_clone.remote {
                    // Try to get from remote
                    match remote.get(&hash_clone).await {
                        Ok(Some(data)) => {
                            // Successfully fetched, store in local cache
                            if let Err(e) = cache_clone.local.put(&hash_clone, &data) {
                                eprintln!("⚠️ Prefetch write error for {}: {}", hash_clone, e);
                            } else {
                                println!("   📥 Prefetched {} from remote", &hash_clone[..8]);
                            }
                        }
                        Ok(None) => {
                            // Not in remote cache, which is fine
                        }
                        Err(e) => {
                            eprintln!("⚠️ Prefetch fetch error for {}: {}", hash_clone, e);
                        }
                    }
                }
            });
        }
    }
}

impl HybridCache {
    pub async fn upload_manifest_and_files(
        &self,
        manifest: &crate::cache::utils::ArtifactManifest,
        base_dir: &std::path::Path,
    ) -> Result<()> {
        // 1. Upload the manifest itself
        let manifest_json = serde_json::to_vec(manifest)?;
        let manifest_hash = manifest.hash();
        self.put_artifact(&manifest_hash, &manifest_json).await?;

        // 2. Upload all files referenced by the manifest
        for file in &manifest.files {
            let file_path = if base_dir.is_file() {
                base_dir.to_path_buf()
            } else {
                base_dir.join(&file.path)
            };
            if file_path.exists() {
                let data = std::fs::read(&file_path)?;
                self.put_artifact(&file.hash, &data).await?;
            }
        }

        Ok(())
    }
}