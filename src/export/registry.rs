use crate::export::manifest::OCIManifest;
use anyhow::{Context, Result};
use reqwest::blocking::Client;

use std::fs;
use std::path::Path;

pub struct RegistryClient {
    client: Client,
    base_url: String, // e.g., https://index.docker.io/v2
    repo: String,     // e.g., library/ubuntu
    token: Option<String>,
}

impl RegistryClient {
    pub fn new(registry: &str, repo: &str) -> Self {
        let base_url = if registry.contains("://") {
            format!("{}/v2", registry)
        } else {
            format!("https://{}/v2", registry)
        };

        Self {
            client: Client::new(),
            base_url,
            repo: repo.to_string(),
            token: None,
        }
    }

    pub fn set_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
    }

    /// Push an OCI layout directory to the registry
    pub fn push(&self, layout_dir: &Path) -> Result<()> {
        let _span = crate::oci_span!("push", &self.base_url, 0); // layer_count will be updated

        println!("🚀 Pushing image to {}/{}...", self.base_url, self.repo);

        // 1. Read index.json to find the manifest
        let index_path = layout_dir.join("index.json");
        let index_content = fs::read_to_string(&index_path)?;
        let index: serde_json::Value = serde_json::from_str(&index_content)?;
        let manifest_descriptor = &index["manifests"][0];
        let manifest_digest = manifest_descriptor["digest"]
            .as_str()
            .context("No manifest found in index.json")?;

        // 2. Read the manifest to find layers
        let manifest_path = layout_dir
            .join("blobs")
            .join("sha256")
            .join(&manifest_digest[7..]);
        let manifest_content = fs::read_to_string(&manifest_path)?;
        let manifest: OCIManifest = serde_json::from_str(&manifest_content)?;

        // 3. Push layers
        for layer in &manifest.layers {
            let layer_path = layout_dir
                .join("blobs")
                .join("sha256")
                .join(&layer.digest[7..]);
            self.upload_blob(&layer.digest, &layer_path)?;
        }

        // 4. Push config
        let config_path = layout_dir
            .join("blobs")
            .join("sha256")
            .join(&manifest.config.digest[7..]);
        self.upload_blob(&manifest.config.digest, &config_path)?;

        // 5. Push manifest
        self.upload_manifest(manifest_digest, &manifest_content)?;

        println!("✅ Image pushed successfully!");
        Ok(())
    }

    /// Pull an image from the registry into an OCI layout directory
    pub fn pull(&self, tag: &str, output_dir: &Path) -> Result<()> {
        println!(
            "📥 Pulling image {}/{} : {}...",
            self.base_url, self.repo, tag
        );
        fs::create_dir_all(output_dir.join("blobs").join("sha256"))?;

        // 1. Fetch Manifest
        let manifest_url = format!("{}/{}/manifests/{}", self.base_url, self.repo, tag);
        let mut rb = self
            .client
            .get(&manifest_url)
            .header("Accept", "application/vnd.oci.image.manifest.v1+json");
        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }

        let resp = rb.send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch manifest: {}", resp.status());
        }

        let manifest_content = resp.text()?;
        let manifest: OCIManifest = serde_json::from_str(&manifest_content)?;
        let manifest_digest = format!(
            "sha256:{}",
            crate::export::utils::sha256_string(&manifest_content)
        );

        // Save manifest
        fs::write(
            output_dir
                .join("blobs")
                .join("sha256")
                .join(&manifest_digest[7..]),
            &manifest_content,
        )?;

        // 2. Fetch Config
        self.download_blob(&manifest.config.digest, output_dir)?;

        // 3. Fetch Layers
        for layer in &manifest.layers {
            self.download_blob(&layer.digest, output_dir)?;
        }

        // 4. Create index.json
        let index = crate::export::manifest::OCIIndex {
            schema_version: 2,
            manifests: vec![crate::export::manifest::OCIDescriptor {
                media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
                digest: manifest_digest,
                size: manifest_content.len() as u64,
            }],
        };
        fs::write(
            output_dir.join("index.json"),
            serde_json::to_string_pretty(&index)?,
        )?;

        // 5. Create oci-layout
        fs::write(
            output_dir.join("oci-layout"),
            r#"{"imageLayoutVersion": "1.0.0"}"#,
        )?;

        println!("✅ Image pulled successfully to {}", output_dir.display());
        Ok(())
    }

    fn download_blob(&self, digest: &str, output_dir: &Path) -> Result<()> {
        println!("   📥 Downloading blob: {}...", status_hash(digest));
        let url = format!("{}/{}/blobs/{}", self.base_url, self.repo, digest);
        let mut rb = self.client.get(&url);
        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }

        let mut resp = rb.send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to download blob {}: {}", digest, resp.status());
        }

        let blob_path = output_dir.join("blobs").join("sha256").join(&digest[7..]);
        let mut file = fs::File::create(blob_path)?;
        std::io::copy(&mut resp, &mut file)?;

        Ok(())
    }

    fn upload_blob(&self, digest: &str, path: &Path) -> Result<()> {
        // Check if blob exists first (cross-repo mount or just skip)
        if self.blob_exists(digest)? {
            println!("   (skip blob: {} already exists)", &status_hash(digest));
            return Ok(());
        }

        println!("   📤 Uploading blob: {}...", status_hash(digest));

        // Initiating upload
        let url = format!("{}/{}/blobs/uploads/", self.base_url, self.repo);
        let mut rb = self.client.post(&url);
        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }

        let resp = rb.send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to initiate blob upload: {}", resp.status());
        }

        let location = resp
            .headers()
            .get("Location")
            .context("No Location header in upload initiation")?
            .to_str()?;

        // Re-construct the URL if it's relative
        let final_url = if location.starts_with('/') {
            // Extract host from base_url
            let host = self.base_url.split("/v2").next().unwrap_or(&self.base_url);
            format!("{}{}&digest={}", host, location, digest)
        } else {
            format!("{}&digest={}", location, digest)
        };

        let file_content = fs::read(path)?;
        let mut rb = self.client.put(&final_url).body(file_content);
        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }

        let resp = rb.send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to upload blob: {}", resp.status());
        }

        Ok(())
    }

    fn blob_exists(&self, digest: &str) -> Result<bool> {
        let url = format!("{}/{}/blobs/{}", self.base_url, self.repo, digest);
        let mut rb = self.client.head(&url);
        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }
        let resp = rb.send()?;
        Ok(resp.status().is_success())
    }

    fn upload_manifest(&self, digest: &str, content: &str) -> Result<()> {
        println!("   📜 Uploading manifest: {}...", status_hash(digest));
        let url = format!("{}/{}/manifests/latest", self.base_url, self.repo); // Using 'latest' tag

        let mut rb = self
            .client
            .put(&url)
            .header("Content-Type", "application/vnd.oci.image.manifest.v1+json")
            .body(content.to_string());

        if let Some(ref t) = self.token {
            rb = rb.bearer_auth(t);
        }

        let resp = rb.send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to upload manifest: {}", resp.status());
        }

        Ok(())
    }
}

fn status_hash(digest: &str) -> &str {
    if digest.len() > 15 {
        &digest[7..15]
    } else {
        digest
    }
}
