use crate::env::EnvFingerprint;
use crate::graph::BuildGraph;

#[allow(dead_code)]
pub fn detect_changes(graph: &mut BuildGraph) {
    for node in &mut graph.nodes {
        node.dirty = true;
    }
}

#[allow(dead_code)]
pub fn propagate_dirty(graph: &mut BuildGraph) {
    let node_count = graph.nodes.len();
    for i in 0..node_count {
        if !graph.nodes[i].dirty {
            for dep in &graph.nodes[i].deps {
                if *dep < node_count && graph.nodes[*dep].dirty {
                    graph.nodes[i].dirty = true;
                    break;
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn compute_composite_hashes(graph: &mut BuildGraph, _env_fp: &EnvFingerprint) {
    for node in &mut graph.nodes {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(node.content.as_bytes());
        node.hash = hasher.finalize().to_hex().to_string();
    }
}

#[allow(dead_code)]
pub fn propagate_manifests(graph: &mut BuildGraph) -> std::collections::HashMap<String, serde_json::Value> {
    let mut manifests = std::collections::HashMap::new();
    for node in &mut graph.nodes {
        node.metadata.input_manifest_hash = Some(node.hash.clone());
        // Assume manifest is the metadata
        manifests.insert(node.hash.clone(), serde_json::to_value(&node.metadata).unwrap());
    }
    manifests
}
