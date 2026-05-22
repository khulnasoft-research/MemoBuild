use crate::export::{
    config, layer,
    manifest::{OCIDescriptor, OCIIndex, OCIManifest},
    utils,
};
use crate::graph::Node;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub struct OciExporter {
    output_dir: PathBuf,
    layers: Vec<layer::LayerInfo>,
}

impl OciExporter {
    pub fn new<P: AsRef<Path>>(output_dir: P) -> Self {
        let output_dir = output_dir.as_ref().to_path_buf();
        Self {
            output_dir,
            layers: Vec::new(),
        }
    }

    pub fn create_layer(&self, node: &Node) -> Result<layer::LayerInfo> {
        layer::create_layer_tar(&self.output_dir, node)
    }

    pub fn add_layer(&mut self, layer_info: layer::LayerInfo) -> Result<()> {
        self.layers.push(layer_info);
        Ok(())
    }

    pub fn write_manifest(
        &self,
        graph: &crate::graph::BuildGraph,
        reproducible: bool,
    ) -> Result<PathBuf> {
        fs::create_dir_all(&self.output_dir)?;
        let blobs_dir = self.output_dir.join("blobs").join("sha256");
        fs::create_dir_all(&blobs_dir)?;

        // 1. Create config
        let oci_config = config::create_config(graph, &self.layers, reproducible);
        let config_json = serde_json::to_string_pretty(&oci_config)?;
        let config_digest = format!("sha256:{}", utils::sha256_string(&config_json));

        fs::write(blobs_dir.join(&config_digest[7..]), &config_json)?;

        // 2. Create manifest
        let manifest = OCIManifest {
            schema_version: 2,
            media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
            config: OCIDescriptor {
                media_type: "application/vnd.oci.image.config.v1+json".to_string(),
                digest: config_digest,
                size: config_json.len() as u64,
            },
            layers: self
                .layers
                .iter()
                .map(|l| OCIDescriptor {
                    media_type: "application/vnd.oci.image.layer.v1.tar+gzip".to_string(),
                    digest: l.digest.clone(),
                    size: l.size,
                })
                .collect(),
        };

        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        let manifest_digest = format!("sha256:{}", utils::sha256_string(&manifest_json));
        fs::write(blobs_dir.join(&manifest_digest[7..]), &manifest_json)?;

        // 3. Create index.json
        let index = OCIIndex {
            schema_version: 2,
            manifests: vec![OCIDescriptor {
                media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
                digest: manifest_digest,
                size: manifest_json.len() as u64,
            }],
        };
        fs::write(
            self.output_dir.join("index.json"),
            serde_json::to_string_pretty(&index)?,
        )?;

        // 4. Create oci-layout
        fs::write(
            self.output_dir.join("oci-layout"),
            r#"{"imageLayoutVersion": "1.0.0"}"#,
        )?;

        println!(
            "✅ OCI Image manifest written to: {}",
            self.output_dir.display()
        );
        Ok(self.output_dir.clone())
    }
}
