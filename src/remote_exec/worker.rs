use crate::cache::HybridCache;
use crate::graph::{Node, NodeKind, NodeMetadata};
use crate::remote_exec::{ActionRequest, ActionResult, Digest, ExecutionMetadata, RemoteExecutor};
use crate::sandbox::Sandbox;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub struct WorkerNode {
    pub id: String,
    pub cache: Arc<HybridCache>,
    pub sandbox: Arc<dyn Sandbox>,
}

impl WorkerNode {
    pub fn new(id: &str, cache: Arc<HybridCache>, sandbox: Arc<dyn Sandbox>) -> Self {
        Self {
            id: id.to_string(),
            cache,
            sandbox,
        }
    }
}

#[async_trait]
impl RemoteExecutor for WorkerNode {
    async fn execute(&self, action: ActionRequest) -> Result<ActionResult> {
        let start_time = Instant::now();
        println!("👷 [Worker {}] Received execution request", self.id);

        // 1. Prepare a virtual Node for the sandbox
        // In REAPI, the inputs are defined by the input_root_digest.
        // For our MVP, we'll map this back to MemoBuild's expectations.
        let node = Node {
            id: 0,
            name: format!("remote-action-{}", &action.input_root_digest.hash[..8]),
            kind: NodeKind::Run,
            content: action.command.join(" "),
            env: action.env.clone(),
            hash: action.input_root_digest.hash.clone(),
            dirty: true,
            deps: Vec::new(),
            source_path: None,
            cache_hit: false,
            metadata: NodeMetadata::default(),
        };

        // 2. Prepare Sandbox
        let env = self
            .sandbox
            .prepare(&node)
            .await
            .context("Failed to prepare sandbox for remote execution")?;

        // 2b. Reconstruct Inputs from Manifest
        if let Ok(Some(manifest_data)) = self
            .cache
            .get_artifact(&action.input_root_digest.hash)
            .await
        {
            if let Ok(manifest) =
                serde_json::from_slice::<crate::cache_utils_exe::ArtifactManifest>(&manifest_data)
            {
                println!(
                    "   📥 [Worker {}] Reconstructing {} files...",
                    self.id,
                    manifest.files.len()
                );
                let cache_clone = self.cache.clone();
                manifest
                    .reconstruct(&env.workspace_dir, move |h| {
                        let cache = cache_clone.clone();
                        async move { cache.get_artifact(&h).await }
                    })
                    .await
                    .context("Failed to reconstruct inputs from manifest")?;
            }
        }

        // 3. Execute
        let exec_result = self
            .sandbox
            .execute(&env, &node)
            .await
            .context("Failed to execute command in remote sandbox")?;

        // 4. Capture Outputs
        let mut output_files = HashMap::new();

        for path in &action.output_files {
            let full_path = env.workspace_dir.join(path);
            if full_path.exists() && full_path.is_file() {
                match std::fs::read(&full_path) {
                    Ok(data) => {
                        let hash = blake3::hash(&data).to_string();
                        let digest = Digest {
                            hash: hash.clone(),
                            size_bytes: data.len() as i64,
                        };

                        // Upload to cache
                        if let Err(e) = self.cache.put_artifact(&hash, &data).await {
                            eprintln!(
                                "⚠️ [Worker {}] Failed to upload output {}: {}",
                                self.id, path, e
                            );
                        } else {
                            output_files.insert(path.clone(), digest);
                            println!(
                                "   📤 [Worker {}] Uploaded output: {} ({})",
                                self.id,
                                path,
                                &hash[..8]
                            );
                        }
                    }
                    Err(e) => eprintln!(
                        "⚠️ [Worker {}] Failed to read output {}: {}",
                        self.id, path, e
                    ),
                }
            }
        }

        // 5. Cleanup
        self.sandbox.cleanup(&env).await.ok();

        let end_time = Instant::now();

        Ok(ActionResult {
            output_files,
            exit_code: exec_result.exit_code,
            stdout_raw: exec_result.stdout,
            stderr_raw: exec_result.stderr,
            execution_metadata: ExecutionMetadata {
                worker_id: self.id.clone(),
                queued_timestamp: None,
                worker_start_timestamp: Some(start_time.elapsed().as_millis() as i64),
                worker_completed_timestamp: Some(end_time.elapsed().as_millis() as i64),
            },
        })
    }
}
