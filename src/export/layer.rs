use crate::export::utils::{sha256_bytes, sha256_string};
use crate::graph::Node;
use anyhow::Result;

use flate2::Compression;
use std::fs::{self, File};
use std::path::Path;
use tar::Builder;

#[derive(Debug, Clone)]
pub struct LayerInfo {
    pub digest: String,
    pub size: u64,
    pub diff_id: String,
}

pub fn create_layer_tar(output_dir: &Path, node: &Node) -> Result<LayerInfo> {
    let layers_dir = output_dir.join("blobs").join("sha256");
    fs::create_dir_all(&layers_dir)?;

    let layer_filename = format!("layer-{}.tar.gz", node.id);
    let layer_path = layers_dir.join(&layer_filename);

    let file = File::create(&layer_path)?;
    // Use GzBuilder to fix gzip header mtime to 0 for reproducibility
    let encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(file, Compression::default());
    let mut tar = Builder::new(encoder);

    // For now, we add a marker file representing the layer content.
    // In a real execution engine, this would include the actual filesystem diff.
    let content = format!(
        "Node: {}\nHash: {}\nEnv: {:?}",
        node.name, node.hash, node.env
    );
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();

    tar.append_data(
        &mut header,
        format!("memobuild/node-{}.txt", node.id),
        content.as_bytes(),
    )?;

    tar.finish()?;

    let layer_content = fs::read(&layer_path)?;
    let digest = format!("sha256:{}", sha256_bytes(&layer_content));
    let size = layer_content.len() as u64;
    let diff_id = format!("sha256:{}", sha256_string(&content));

    // Rename to its digest-based name for OCI layout
    let digest_path = layers_dir.join(&digest[7..]);
    fs::rename(layer_path, digest_path)?;

    Ok(LayerInfo {
        digest,
        size,
        diff_id,
    })
}
