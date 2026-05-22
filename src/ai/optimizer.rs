use crate::env::EnvFingerprint;
use crate::graph::{BuildGraph, NodeKind};

pub struct BuildOptimizer;

impl Default for BuildOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOptimizer {
    pub fn new() -> Self {
        Self
    }

    pub fn optimize_graph(&self, graph: &mut BuildGraph, _env_fp: &EnvFingerprint) {
        println!("   🧠 Applying ML-based build optimization...");

        for node in &mut graph.nodes {
            // Heuristic: RUN nodes that appear to be independent or CPU-bound
            // should be prioritized and marked as parallelizable.
            if let NodeKind::Run = &node.kind {
                if node.content.contains("test") || node.content.contains("build") {
                    println!(
                        "      ⚡ Optimizing node {}: '{}' - Setting high priority",
                        node.id, node.name
                    );
                    node.metadata.priority = 10;
                    node.metadata.parallelizable = true;
                }
            }

            // Predicting execution time based on content (Simulated ML)
            let predicted_ms = self.predict_execution_time(&node.content);
            node.metadata.execution_time_ms = Some(predicted_ms);
        }

        // Re-order levels if needed (BuildGraph.levels() uses these properties)
    }

    fn predict_execution_time(&self, content: &str) -> u64 {
        if content.contains("npm install") || content.contains("cargo build") {
            60000 // 1 minute
        } else if content.contains("test") {
            30000 // 30 seconds
        } else {
            5000 // 5 seconds
        }
    }
}
