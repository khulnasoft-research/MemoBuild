use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum NodeKind {
    From,
    Run,
    Copy {
        src: PathBuf,
        dst: PathBuf,
    },
    Env,
    Workdir,
    Cmd,
    Git {
        url: String,
        target: PathBuf,
    },
    // Docker Extension Nodes
    RunExtend {
        command: String,
        parallelizable: bool,
    },
    CopyExtend {
        src: PathBuf,
        dst: PathBuf,
        tags: Vec<String>,
    },
    CustomHook {
        hook_name: String,
        params: Vec<String>,
    },
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: usize,
    pub name: String,
    pub content: String,
    pub kind: NodeKind,
    pub hash: String,
    pub dirty: bool,
    pub deps: Vec<usize>,
    /// Set for COPY nodes — the source path to hash from the filesystem
    pub source_path: Option<PathBuf>,
    pub env: std::collections::HashMap<String, String>,
    pub cache_hit: bool,
    /// Additional metadata for rich node tracking
    pub metadata: NodeMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NodeMetadata {
    /// Timestamp when node was last executed
    pub last_executed: Option<std::time::SystemTime>,
    /// Execution duration in milliseconds
    pub execution_time_ms: Option<u64>,
    /// Whether this node can be executed in parallel
    pub parallelizable: bool,
    /// Node priority for execution scheduling
    pub priority: u8,
    /// Custom tags for node categorization
    pub tags: Vec<String>,
    /// Content hash of source files (for COPY nodes)
    pub source_content_hash: Option<String>,
    /// Hash of the ArtifactManifest for inputs (used for remote reconstruction)
    pub input_manifest_hash: Option<String>,
    /// Hash of the ArtifactManifest for outputs (what this node produced)
    pub output_manifest_hash: Option<String>,
    /// AI-detected extra dependencies (source paths)
    pub extra_source_paths: Vec<std::path::PathBuf>,
}

impl Node {
    /// Computes a unique key for the node based on its kind, content, dependencies, and optional context.
    /// This is the heart of incremental builds and content-addressed identities.
    pub fn compute_node_key(
        &self,
        dep_hashes: &[String],
        context_hash: Option<&str>,
        env_fingerprint: Option<&crate::env::EnvFingerprint>,
    ) -> String {
        let mut hasher = blake3::Hasher::new();

        // 1. Hash the kind and instruction content
        hasher.update(format!("{:?}", self.kind).as_bytes());
        hasher.update(self.content.as_bytes());

        // 2. Hash environment variables for ENV nodes
        if !self.env.is_empty() {
            let mut env_keys: Vec<_> = self.env.keys().collect();
            env_keys.sort(); // Ensure deterministic ordering
            for key in env_keys {
                if let Some(value) = self.env.get(key) {
                    hasher.update(format!("{}={}", key, value).as_bytes());
                }
            }
        }

        // 3. Hash context if present (e.g. filesystem hash for COPY)
        if let Some(ch) = context_hash {
            hasher.update(ch.as_bytes());
        }

        // 4. Hash source content hash if available (for COPY nodes)
        if let Some(source_hash) = &self.metadata.source_content_hash {
            hasher.update(source_hash.as_bytes());
        }

        // 5. Hash dependencies to ensure propagation
        let mut sorted_dep_hashes = dep_hashes.to_vec();
        sorted_dep_hashes.sort(); // Ensure deterministic ordering
        for dep_hash in sorted_dep_hashes {
            hasher.update(dep_hash.as_bytes());
        }

        // 6. Hash metadata that affects execution
        hasher.update(format!("parallelizable={}", self.metadata.parallelizable).as_bytes());
        hasher.update(format!("priority={}", self.metadata.priority).as_bytes());

        // 7. Hash environment fingerprint for global determinism
        if let Some(fp) = env_fingerprint {
            hasher.update(fp.hash().as_bytes());
        }

        hasher.finalize().to_hex().to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildGraph {
    pub nodes: Vec<Node>,
}

impl BuildGraph {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl BuildGraph {
    /// Get nodes in topological order for execution
    pub fn topological_order(&self) -> Vec<usize> {
        let mut visited = vec![false; self.nodes.len()];
        let mut stack = Vec::new();

        for i in 0..self.nodes.len() {
            if !visited[i] {
                self.dfs_topo(i, &mut visited, &mut stack);
            }
        }

        stack.reverse();
        stack
    }

    fn dfs_topo(&self, node: usize, visited: &mut Vec<bool>, stack: &mut Vec<usize>) {
        visited[node] = true;

        for &dep in &self.nodes[node].deps {
            if dep < self.nodes.len() && !visited[dep] {
                self.dfs_topo(dep, visited, stack);
            }
        }

        stack.push(node);
    }

    /// Group nodes into levels that can be executed in parallel
    pub fn levels(&self) -> Vec<Vec<usize>> {
        let mut node_levels = vec![0; self.nodes.len()];
        let order = self.topological_order();

        for &node_id in &order {
            let mut max_dep_level = 0;
            for &dep in &self.nodes[node_id].deps {
                if dep < self.nodes.len() {
                    max_dep_level = std::cmp::max(max_dep_level, node_levels[dep] + 1);
                }
            }
            node_levels[node_id] = max_dep_level;
        }

        let max_level = node_levels.iter().max().cloned().unwrap_or(0);
        let mut result = vec![Vec::new(); max_level + 1];

        for (node_id, &level) in node_levels.iter().enumerate() {
            result[level].push(node_id);
        }

        result
    }
}
