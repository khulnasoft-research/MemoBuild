use crate::export::layer::LayerInfo;
use crate::graph::BuildGraph;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct OCIConfig {
    pub architecture: String,
    pub os: String,
    pub config: OCIImageConfig,
    pub rootfs: OCIRootFS,
    pub history: Vec<OCIHistory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OCIImageConfig {
    #[serde(rename = "Env")]
    pub env: Vec<String>,
    #[serde(rename = "Cmd")]
    pub cmd: Option<Vec<String>>,
    #[serde(rename = "WorkingDir")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OCIRootFS {
    #[serde(rename = "type")]
    pub fs_type: String,
    pub diff_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OCIHistory {
    pub created: String,
    pub created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_layer: Option<bool>,
}

pub fn create_config(graph: &BuildGraph, layers: &[LayerInfo], reproducible: bool) -> OCIConfig {
    let mut env = Vec::new();
    for node in &graph.nodes {
        for (k, v) in &node.env {
            env.push(format!("{}={}", k, v));
        }
    }

    let timestamp = if reproducible {
        "1970-01-01T00:00:00Z".to_string()
    } else {
        Utc::now().to_rfc3339()
    };

    OCIConfig {
        architecture: "amd64".to_string(),
        os: "linux".to_string(),
        config: OCIImageConfig {
            env,
            cmd: Some(vec!["/bin/sh".to_string()]),
            working_dir: Some("/".to_string()),
        },
        rootfs: OCIRootFS {
            fs_type: "layers".to_string(),
            diff_ids: layers.iter().map(|l| l.diff_id.clone()).collect(),
        },
        history: graph
            .nodes
            .iter()
            .map(|n| OCIHistory {
                created: timestamp.clone(),
                created_by: format!("MemoBuild: {}", n.name),
                empty_layer: Some(false),
            })
            .collect(),
    }
}
