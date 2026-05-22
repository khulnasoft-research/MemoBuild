use crate::cache::hybrid::HybridCache;
use crate::graph::BuildGraph;
use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::Instant;

/// Incremental executor that supports parallel execution and selective rebuilds
pub struct IncrementalExecutor {
    cache: Arc<HybridCache>,
    execution_stats: ExecutionStats,
    observer: Option<Arc<dyn crate::dashboard::BuildObserver>>,
    reproducible: bool,
    dry_run: bool,
    sandbox: Arc<dyn crate::sandbox::Sandbox>,
    remote_executor: Option<Arc<dyn crate::remote_exec::RemoteExecutor>>,
}

#[derive(Debug, Default, Clone)]
pub struct ExecutionStats {
    pub total_nodes: usize,
    pub executed_nodes: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub parallel_levels: usize,
    pub total_execution_time_ms: u64,
}

impl IncrementalExecutor {
    pub fn new(cache: Arc<HybridCache>) -> Self {
        Self {
            cache,
            execution_stats: ExecutionStats::default(),
            observer: None,
            reproducible: false,
            dry_run: false,
            sandbox: Arc::new(crate::sandbox::local::LocalSandbox::new(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )),
            remote_executor: None,
        }
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_remote_executor(
        mut self,
        exec: Arc<dyn crate::remote_exec::RemoteExecutor>,
    ) -> Self {
        self.remote_executor = Some(exec);
        self
    }

    pub fn with_sandbox(mut self, sandbox: Arc<dyn crate::sandbox::Sandbox>) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub fn with_reproducible(mut self, reproducible: bool) -> Self {
        self.reproducible = reproducible;
        self
    }

    pub fn with_observer(mut self, observer: Arc<dyn crate::dashboard::BuildObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Execute the build graph with parallel and incremental capabilities
    pub async fn execute(&mut self, graph: &mut BuildGraph) -> Result<ExecutionStats> {
        let start_time = Instant::now();

        // Reset stats
        self.execution_stats = ExecutionStats::default();
        self.execution_stats.total_nodes = graph.nodes.len();

        // Get execution levels for parallel processing
        let levels = graph.levels();
        self.execution_stats.parallel_levels = levels.len();

        if let Some(ref obs) = self.observer {
            obs.on_event(crate::dashboard::BuildEvent::BuildStarted {
                total_nodes: self.execution_stats.total_nodes,
            });
        }

        println!(
            "🚀 Starting incremental execution with {} levels",
            levels.len().to_string().cyan()
        );

        let pb = ProgressBar::new(self.execution_stats.total_nodes as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );

        for (level_idx, level) in levels.iter().enumerate() {
            if level.is_empty() {
                continue;
            }

            println!(" Executing level {}: {} nodes", level_idx, level.len());

            let (parallel_nodes, sequential_nodes): (Vec<_>, Vec<_>) = level
                .iter()
                .partition(|&&node_id| graph.nodes[node_id].metadata.parallelizable);

            // Execute parallel nodes first
            if !parallel_nodes.is_empty() {
                self.execute_parallel_nodes(graph, &parallel_nodes, &pb)
                    .await?;
            }

            // Execute sequential nodes
            if !sequential_nodes.is_empty() {
                self.execute_sequential_nodes(graph, &sequential_nodes, &pb)
                    .await?;
            }
            // Finalize execute
        }

        pb.finish_with_message("Execution completed".green().to_string());

        self.execution_stats.total_execution_time_ms = start_time.elapsed().as_millis() as u64;

        if let Some(ref obs) = self.observer {
            obs.on_event(crate::dashboard::BuildEvent::BuildCompleted {
                total_duration_ms: self.execution_stats.total_execution_time_ms,
                cache_hits: self.execution_stats.cache_hits,
                executed_nodes: self.execution_stats.executed_nodes,
            });
        }

        self.print_execution_summary();

        Ok(self.execution_stats.clone())
    }

    /// Execute nodes in parallel
    async fn execute_parallel_nodes(
        &mut self,
        graph: &mut BuildGraph,
        node_ids: &[&usize],
        pb: &ProgressBar,
    ) -> Result<()> {
        pb.set_message(format!("⚡ Executing {} nodes in parallel", node_ids.len()));

        let mut futures = Vec::new();

        for &&node_id in node_ids {
            let node = graph.nodes[node_id].clone();
            let name = node.name.clone();
            let hash = node.hash.clone();
            let dirty = node.dirty;
            let kind = node.kind.clone();
            let cache = self.cache.clone();
            let observer = self.observer.clone();
            let sandbox = self.sandbox.clone();
            let remote_executor = self.remote_executor.clone();
            let reproducible = self.reproducible;
            let dry_run = self.dry_run;

            futures.push(async move {
                if let Some(ref obs) = observer {
                    obs.on_event(crate::dashboard::BuildEvent::NodeStarted {
                        node_id,
                        name: name.clone(),
                    });
                }
                let start_time = Instant::now();
                let result = Self::execute_node_logic(
                    cache,
                    node_id,
                    &name,
                    &hash,
                    dirty,
                    &kind,
                    reproducible,
                    dry_run,
                    sandbox,
                    remote_executor,
                    &node,
                )
                .await;
                let execution_time = start_time.elapsed().as_millis() as u64;

                if let Some(ref obs) = observer {
                    match &result {
                        Ok((_, cache_hit)) => {
                            obs.on_event(crate::dashboard::BuildEvent::NodeCompleted {
                                node_id,
                                name: name.clone(),
                                duration_ms: execution_time,
                                cache_hit: *cache_hit,
                            })
                        }
                        Err(e) => obs.on_event(crate::dashboard::BuildEvent::NodeFailed {
                            node_id,
                            name: name.clone(),
                            error: e.to_string(),
                        }),
                    }
                }
                (node_id, result, execution_time)
            });
        }

        let results = futures::future::join_all(futures).await;

        // Update graph status and stats
        for (node_id, result, execution_time) in results {
            let (dirty, cache_hit) = result?;

            graph.nodes[node_id].dirty = dirty;
            graph.nodes[node_id].cache_hit = cache_hit;
            graph.nodes[node_id].metadata.last_executed = Some(std::time::SystemTime::now());
            graph.nodes[node_id].metadata.execution_time_ms = Some(execution_time);

            if cache_hit {
                self.execution_stats.cache_hits += 1;
            } else {
                self.execution_stats.cache_misses += 1;
                self.execution_stats.executed_nodes += 1;
            }
            pb.inc(1);
        }

        Ok(())
    }

    /// Execute nodes sequentially
    async fn execute_sequential_nodes(
        &mut self,
        graph: &mut BuildGraph,
        node_ids: &[&usize],
        pb: &ProgressBar,
    ) -> Result<()> {
        pb.set_message(format!(
            "🔧 Executing {} nodes sequentially",
            node_ids.len()
        ));

        for &&node_id in node_ids {
            let start_time = Instant::now();
            let node = &graph.nodes[node_id];

            if let Some(ref obs) = self.observer {
                obs.on_event(crate::dashboard::BuildEvent::NodeStarted {
                    node_id,
                    name: node.name.clone(),
                });
            }

            let result = Self::execute_node_logic(
                self.cache.clone(),
                node_id,
                &node.name,
                &node.hash,
                node.dirty,
                &node.kind,
                self.reproducible,
                self.dry_run,
                self.sandbox.clone(),
                self.remote_executor.clone(),
                node,
            )
            .await;

            let execution_time = start_time.elapsed().as_millis() as u64;

            if let Some(ref obs) = self.observer {
                match &result {
                    Ok((_, cache_hit)) => {
                        obs.on_event(crate::dashboard::BuildEvent::NodeCompleted {
                            node_id,
                            name: node.name.clone(),
                            duration_ms: execution_time,
                            cache_hit: *cache_hit,
                        })
                    }
                    Err(e) => obs.on_event(crate::dashboard::BuildEvent::NodeFailed {
                        node_id,
                        name: node.name.clone(),
                        error: e.to_string(),
                    }),
                }
            }

            let (dirty, cache_hit) = result?;

            graph.nodes[node_id].dirty = dirty;
            graph.nodes[node_id].cache_hit = cache_hit;
            graph.nodes[node_id].metadata.last_executed = Some(std::time::SystemTime::now());
            graph.nodes[node_id].metadata.execution_time_ms = Some(execution_time);

            if cache_hit {
                self.execution_stats.cache_hits += 1;
            } else {
                self.execution_stats.cache_misses += 1;
                self.execution_stats.executed_nodes += 1;
            }
            pb.inc(1);
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_node_logic(
        cache: Arc<HybridCache>,
        _node_id: usize,
        name: &str,
        hash: &str,
        dirty: bool,
        _kind: &crate::graph::NodeKind,
        reproducible: bool,
        dry_run: bool,
        sandbox: Arc<dyn crate::sandbox::Sandbox>,
        remote_executor: Option<Arc<dyn crate::remote_exec::RemoteExecutor>>,
        node: &crate::graph::Node,
    ) -> Result<(bool, bool)> {
        // 1. Check cache first
        match cache.get_artifact(hash).await {
            Ok(Some(_data)) => {
                // Return silently, progress bar handles message visually without spam
                return Ok((false, true));
            }
            Err(e) => eprintln!("{}", format!("⚠️ Cache error for {}: {}", name, e).red()),
            _ => {}
        }

        if dry_run {
            println!(
                "{}",
                format!("Dry-run mode, skipping execution for {}", name).yellow()
            );
            return Ok((dirty, false));
        }

        // Check if node type needs actual execution in build farm
        let is_runnable = matches!(
            node.kind,
            crate::graph::NodeKind::Run
                | crate::graph::NodeKind::RunExtend { .. }
                | crate::graph::NodeKind::CustomHook { .. }
                | crate::graph::NodeKind::Git { .. }
        );

        let mut artifact_data = if is_runnable {
            if let Some(remote) = remote_executor.as_ref() {
                // Ensure input manifest and required files are in CAS
                if let Some(ref _manifest_hash) = node.metadata.input_manifest_hash {
                    // If it's a COPY node, we can re-generate and upload
                    if let Some(ref path) = node.source_path {
                        if let Ok(manifest) = crate::cache::utils::ArtifactManifest::from_dir(path) {
                            println!("📤 Uploading input manifest for {}...", name);
                            cache.upload_manifest_and_files(&manifest, path).await?;
                        }
                    } else {
                        // For RUN nodes, the manifest was built from parents.
                        // We should ensure the manifest itself is in the CAS.
                        // (The files should already be there from previous steps' put_artifact)
                        // TODO: Implement manifest persistence across steps if needed
                    }
                }

                println!("📡 [RemoteExec] Dispatching node {} to build farm", name);
                let action = crate::remote_exec::ActionRequest {
                    command: vec!["/bin/sh".into(), "-c".into(), node.content.clone()],
                    env: node.env.clone(),
                    input_root_digest: crate::remote_exec::Digest {
                        hash: node
                            .metadata
                            .input_manifest_hash
                            .clone()
                            .unwrap_or_else(|| hash.to_string()),
                        size_bytes: 0, // Placeholder
                    },
                    timeout: std::time::Duration::from_secs(
                        crate::constants::DEFAULT_REMOTE_EXECUTION_TIMEOUT_SECS,
                    ),
                    platform_properties: std::collections::HashMap::new(),
                    output_files: Vec::new(),
                    output_directories: Vec::new(),
                };

                let result = remote.execute(action).await?;
                if result.exit_code != 0 {
                    anyhow::bail!(
                        "Remote execution failed with exit code {}: {}",
                        result.exit_code,
                        String::from_utf8_lossy(&result.stderr_raw)
                    );
                }
                result.stdout_raw
            } else {
                // Prepare sandbox
                if let crate::graph::NodeKind::RunExtend { command, .. } = &node.kind {
                    println!("⚡ Executing extended RUN: {}", command);
                } else if let crate::graph::NodeKind::CopyExtend { src, dst, .. } = &node.kind {
                    println!(
                        "⚡ Executing extended COPY: {} -> {}",
                        src.display(),
                        dst.display()
                    );
                } else if let crate::graph::NodeKind::CustomHook { hook_name, .. } = &node.kind {
                    println!("⚡ Running custom hook: {}", hook_name);
                }

                let env = sandbox.prepare(node).await?;

                // Execute command
                let exec_result = sandbox.execute(&env, node).await?;

                if exec_result.exit_code != 0 {
                    anyhow::bail!(
                        "Command failed with exit code {}: {}",
                        exec_result.exit_code,
                        String::from_utf8_lossy(&exec_result.stderr)
                    );
                }

                let data = exec_result.stdout;
                sandbox.cleanup(&env).await?;
                data
            }
        } else {
            Vec::new() // Default empty artifact data for non-runnable nodes
        };

        if reproducible {
            artifact_data = crate::reproducible::normalize_artifact(artifact_data)?;
        }

        if let Err(e) = cache.put_artifact(hash, &artifact_data).await {
            eprintln!("⚠️ Cache put error for {}: {}", name, e);
        }

        Ok((false, false))
    }

    /// Print execution summary
    fn print_execution_summary(&self) {
        println!("\n{}", "📊 Execution Summary:".bold().cyan());
        println!("  Total nodes: {}", self.execution_stats.total_nodes);
        println!(
            "  Executed nodes: {}",
            self.execution_stats.executed_nodes.to_string().yellow()
        );
        println!(
            "  Cache hits: {}",
            self.execution_stats.cache_hits.to_string().green()
        );
        println!(
            "  Cache misses: {}",
            self.execution_stats.cache_misses.to_string().red()
        );
        println!(
            "  Parallel levels: {}",
            self.execution_stats.parallel_levels
        );
        println!(
            "  Total time: {}",
            indicatif::HumanDuration(std::time::Duration::from_millis(
                self.execution_stats.total_execution_time_ms
            ))
            .to_string()
            .purple()
        );

        if self.execution_stats.total_nodes > 0 {
            let cache_hit_rate = (self.execution_stats.cache_hits as f64
                / self.execution_stats.total_nodes as f64)
                * 100.0;
            println!("  Cache hit rate: {:.1}%", cache_hit_rate);
        }
    }
}

/// Legacy function for backward compatibility
pub async fn execute_graph(
    graph: &mut BuildGraph,
    cache: Arc<HybridCache>,
    observer: Option<Arc<dyn crate::dashboard::BuildObserver>>,
    reproducible: bool,
) -> Result<()> {
    let mut executor = IncrementalExecutor::new(cache).with_reproducible(reproducible);
    if let Some(obs) = observer {
        executor = executor.with_observer(obs);
    }
    executor.execute(graph).await?;
    Ok(())
}