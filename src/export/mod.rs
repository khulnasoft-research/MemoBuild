pub mod config;
pub mod layer;
pub mod manifest;
pub mod oci_exporter;
pub mod registry;
pub mod utils;

pub use oci_exporter::OciExporter;

use crate::graph::BuildGraph;
use anyhow::Result;
use std::path::PathBuf;

pub fn export_image(graph: &BuildGraph, image_name: &str, reproducible: bool) -> Result<PathBuf> {
    let output_dir = PathBuf::from(".memobuild-output").join(image_name.replace(':', "-"));

    let mut exporter = OciExporter::new(&output_dir);

    for node in &graph.nodes {
        // In this demo, we export all nodes as layers
        let layer_info = exporter.create_layer(node)?;
        exporter.add_layer(layer_info)?;
    }

    exporter.write_manifest(graph, reproducible)
}
