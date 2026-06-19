use blake3;

pub const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks

pub struct ArtifactLayer {
    pub hash: String,
    pub data: Vec<u8>,
}

/// Split an artifact into content-addressed chunks
pub fn split_artifact(data: &[u8]) -> Vec<ArtifactLayer> {
    let mut layers = Vec::new();
    for chunk in data.chunks(CHUNK_SIZE) {
        let hash = blake3::hash(chunk).to_hex().to_string();
        layers.push(ArtifactLayer {
            hash,
            data: chunk.to_vec(),
        });
    }
    layers
}

/// Merge chunks back into a single artifact
pub fn merge_artifact(layers: Vec<Vec<u8>>) -> Vec<u8> {
    let mut data = Vec::with_capacity(layers.iter().map(|l| l.len()).sum());
    for layer in layers {
        data.extend_from_slice(&layer);
    }
    data
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ArtifactManifest {
    pub files: Vec<FileEntry>,
}

impl ArtifactManifest {
    pub fn from_dir(dir: &std::path::Path) -> anyhow::Result<Self> {
        let mut files = Vec::new();
        if dir.is_file() {
            let data = std::fs::read(dir)?;
            let hash = blake3::hash(&data).to_hex().to_string();
            files.push(FileEntry {
                path: dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                hash,
                size: data.len() as u64,
            });
        } else {
            for entry in walkdir::WalkDir::new(dir) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    let rel_path = entry
                        .path()
                        .strip_prefix(dir)?
                        .to_string_lossy()
                        .to_string();
                    let data = std::fs::read(entry.path())?;
                    let hash = blake3::hash(&data).to_hex().to_string();
                    files.push(FileEntry {
                        path: rel_path,
                        hash,
                        size: data.len() as u64,
                    });
                }
            }
        }
        Ok(Self { files })
    }

    pub fn hash(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        blake3::hash(json.as_bytes()).to_hex().to_string()
    }

    pub async fn reconstruct<F, Fut>(
        &self,
        base_dir: &std::path::Path,
        fetcher: F,
    ) -> anyhow::Result<()>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Option<Vec<u8>>>>,
    {
        for file in &self.files {
            let full_path = base_dir.join(&file.path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if let Some(data) = fetcher(file.hash.clone()).await? {
                std::fs::write(full_path, data)?;
            } else {
                anyhow::bail!("Failed to fetch file {} with hash {}", file.path, file.hash);
            }
        }
        Ok(())
    }

    pub fn merge(&mut self, other: &Self) {
        let mut file_map: std::collections::HashMap<String, FileEntry> = self
            .files
            .iter()
            .cloned()
            .map(|f| (f.path.clone(), f))
            .collect();

        for file in &other.files {
            file_map.insert(file.path.clone(), file.clone());
        }

        self.files = file_map.into_values().collect();
    }
}
