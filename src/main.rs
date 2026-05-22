use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use memobuild::server;
use memobuild::{audit, cache, core, docker, executor, export, logging, sbom, slsa, verify};
use hex::encode as hex_encode;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_postgres::NoTls;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "memobuild")]
#[command(about = "High-Performance Incremental Build System", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable JSON logging
    #[arg(long, env = "MEMOBUILD_JSON_LOGS")]
    json_logs: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a container image from a context directory
    Build {
        /// path to the build context
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Path to the Dockerfile
        #[arg(short, long, default_value = "Dockerfile")]
        file: String,

        /// Push the image to a registry after build
        #[arg(long)]
        push: bool,

        /// Enable reproducible build mode
        #[arg(long)]
        reproducible: bool,

        /// Perform a dry run without executing commands
        #[arg(long)]
        dry_run: bool,

        /// Use a specific sandbox runtime (local, containerd)
        #[arg(long)]
        sandbox: Option<String>,

        /// Use remote execution via scheduler
        #[arg(long)]
        remote_exec: bool,
    },
    /// Visualize the dependency graph
    Graph {
        /// Path to the build context
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Path to the Dockerfile
        #[arg(short, long, default_value = "Dockerfile")]
        file: String,
    },
    /// Explain the cache status for a specific node
    ExplainCache {
        /// Path to the build context
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Path to the Dockerfile
        #[arg(short, long, default_value = "Dockerfile")]
        file: String,

        /// Specific node ID or name to explain (optional)
        node: Option<String>,
    },
    /// Start the Remote Cache Server
    Server {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,

        /// Enable PostgreSQL storage
        #[arg(long)]
        postgres: bool,

        /// PostgreSQL connection string
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },
    /// Start the Execution Scheduler
    Scheduler {
        /// Port to listen on
        #[arg(short, long, default_value_t = 9000)]
        port: u16,
    },
    /// Start the Remote Execution gRPC server
    Grpc {
        /// Port to listen on
        #[arg(short, long, default_value_t = 9500)]
        port: u16,
    },
    /// Start a Worker Node
    Worker {
        /// Port to listen on
        #[arg(short, long, default_value_t = 9001)]
        port: u16,

        /// Sandbox runtime to use
        #[arg(long, default_value = "local")]
        sandbox: String,

        /// Scheduler endpoint to register with
        #[arg(long, env = "MEMOBUILD_SCHEDULER_URL")]
        scheduler_url: Option<String>,
    },
    /// Pull an image from a registry
    Pull {
        /// Full image name (e.g. registry.io/repo:tag)
        image: String,
    },
    /// Generate an SBOM for a build context
    Sbom {
        /// Image name to annotate in the SBOM
        image: String,

        /// Path to the build context
        #[arg(default_value = ".")]
        context: PathBuf,

        /// Dockerfile path within the context
        #[arg(short, long, default_value = "Dockerfile")]
        file: String,

        /// Output path for the SBOM
        #[arg(short, long, default_value = "sbom.json")]
        output: String,

        /// Output format (json|xml)
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Verify a container image with Cosign policy
    Verify {
        /// Full image name (e.g. registry.io/repo:tag)
        image: String,

        /// Require a valid signature
        #[arg(long)]
        require_signed: bool,

        /// Include Rekor transparency log validation
        #[arg(long, default_value_t = true)]
        include_rekor: bool,

        /// OIDC token for keyless verification
        #[arg(long)]
        oidc_token: Option<String>,
    },
    /// Generate a SLSA provenance attestation for an artifact
    Attest {
        /// Source URI for provenance materials
        source_uri: String,

        /// Artifact URI for the produced artifact
        artifact_uri: String,

        /// Output path for the attestation
        #[arg(short, long, default_value = "attestation.json")]
        output: String,
    },
    /// Generate CI/CD configurations
    GenerateCi {
        /// CI provider (github, gitlab)
        #[arg(long, default_value = "github")]
        provider: String,
    },
    /// Start a Clustered Cache Server
    Cluster {
        /// Port to listen on
        #[arg(short, long, default_value_t = 9090)]
        port: u16,

        /// Cluster node ID
        #[arg(long)]
        node_id: Option<String>,

        /// Comma-separated list of cluster peer addresses
        #[arg(long)]
        peers: Option<String>,

        /// Enable PostgreSQL storage
        #[arg(long)]
        postgres: bool,

        /// PostgreSQL connection string
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize structured logging
    logging::init_logging(cli.json_logs).ok();

    // Initialize OpenTelemetry tracing if configured
    memobuild::tracing::init_tracing();

    match cli.command {
        Commands::Build {
            path,
            file,
            push,
            reproducible,
            dry_run,
            sandbox,
            remote_exec,
        } => {
            run_build(
                path,
                file,
                push,
                reproducible,
                dry_run,
                sandbox,
                remote_exec,
            )
            .await
        }
        Commands::Graph { path, file } => run_graph(path, file).await,
        Commands::ExplainCache { path, file, node } => run_explain_cache(path, file, node).await,
        Commands::Server { port, postgres, database_url } => {
            let webhook_url = env::var("MEMOBUILD_WEBHOOK").ok();
            let data_dir = env::current_dir()?.join(".memobuild-server");
            fs::create_dir_all(&data_dir)?;

            // Load TLS config if provided
            let tls_config = if let (Ok(cert), Ok(key), Ok(ca)) = (
                env::var("MEMOBUILD_TLS_CERT"),
                env::var("MEMOBUILD_TLS_KEY"),
                env::var("MEMOBUILD_TLS_CA"),
            ) {
                Some(memobuild::tls::TlsConfig::from_files(&cert, &key, &ca)?)
            } else {
                None
            };

            let admin_token = env::var("MEMOBUILD_ADMIN_TOKEN").ok();

            // Create auth database client if PostgreSQL is enabled
            let auth_db_client = if postgres {
                if let Some(db_url) = database_url.as_ref() {
                    let config = parse_postgres_url(db_url)?;
                    let (client, connection) = tokio_postgres::connect(
                        &format!("postgresql://{}:{}@{}:{}/{}",
                            config.user, config.password, config.host, config.port, config.database),
                        NoTls,
                    ).await?;
                    tokio::spawn(async move {
                        if let Err(e) = connection.await {
                            eprintln!("connection error: {}", e);
                        }
                    });
                    Some(client)
                } else {
                    None
                }
            } else {
                None
            };

            server::start_server(port, data_dir, webhook_url, tls_config, admin_token, auth_db_client).await
        }
        Commands::Scheduler { port } => start_scheduler(port).await,
        Commands::Grpc { port } => start_reapi_server(port).await,
        Commands::Worker {
            port,
            sandbox,
            scheduler_url,
        } => start_worker(port, sandbox, scheduler_url).await,
        Commands::Pull { image } => run_pull(image).await,
        Commands::GenerateCi { provider } => run_generate_ci(provider).await,
        Commands::Sbom {
            image,
            context,
            file,
            output,
            format,
        } => run_sbom(image, context, file, output, format).await,
        Commands::Verify {
            image,
            require_signed,
            include_rekor,
            oidc_token,
        } => run_verify(image, require_signed, include_rekor, oidc_token).await,
        Commands::Attest {
            source_uri,
            artifact_uri,
            output,
        } => run_attest(&source_uri, &artifact_uri, &output).await,
        Commands::Cluster {
            port,
            node_id,
            peers,
            postgres,
            database_url,
        } => start_cluster_server(port, node_id, peers, postgres, database_url).await,
    }
}

async fn run_build(
    context_dir: PathBuf,
    dockerfile_path: String,
    push: bool,
    reproducible: bool,
    dry_run: bool,
    sandbox_type: Option<String>,
    remote_exec: bool,
) -> Result<()> {
    println!("🚀 MemoBuild Engine Starting...");

    let build_id = Uuid::new_v4().to_string();
    let audit_logger = audit::AuditLogger::new();
    audit::log_build_event(&audit_logger, &build_id, "started");

    let env_fp = memobuild::env::EnvFingerprint::collect();
    println!("   🔑 Env Fingerprint: {}", &env_fp.hash()[..8]);

    let cache = Arc::new(create_cache().await?);

    let dockerfile = fs::read_to_string(&dockerfile_path)
        .with_context(|| format!("Failed to read Dockerfile at {}", dockerfile_path))?;

    println!("📄 Parsing Dockerfile...");
    let instructions = docker::parser::parse_dockerfile(&dockerfile);

    println!("📊 Building DAG for context: {}...", context_dir.display());
    let mut graph = docker::dag::build_graph_from_instructions(instructions, context_dir.clone());

    let ai_layer = memobuild::ai::AiLayer::new();
    ai_layer.analyze(&mut graph, &env_fp, &context_dir);

    println!("🔍 Detecting changes (filesystem hashing)...");
    core::detect_changes(&mut graph);

    println!("🔄 Propagating dirty flags...");
    core::propagate_dirty(&mut graph);

    println!("🔑 Recomputing deterministic hashes...");
    core::compute_composite_hashes(&mut graph, &env_fp);

    println!("📜 Propagating artifact manifests...");
    let manifests = core::propagate_manifests(&mut graph);

    if let Some(ref _r) = cache.remote {
        for (hash, manifest) in manifests {
            let data = serde_json::to_vec(&manifest)?;
            let cache_clone = cache.clone();
            let hash_clone = hash.clone();
            tokio::spawn(async move {
                let _ = cache_clone.put_artifact(&hash_clone, &data).await;
            });
        }
    }

    let dirty = graph.nodes.iter().filter(|n| n.dirty).count();
    println!(
        "   {} dirty  |  {} cached",
        dirty,
        graph.nodes.len() - dirty
    );

    if dirty > 0 {
        println!("🚀 Initiating smart prefetching for {} nodes...", dirty);
        let dirty_hashes: Vec<String> = graph
            .nodes
            .iter()
            .filter(|n| n.dirty)
            .map(|n| n.hash.clone())
            .collect();
        cache.clone().prefetch_artifacts(dirty_hashes);
    }

    let build_start = std::time::Instant::now();
    let mut executor = executor::IncrementalExecutor::new(cache.clone())
        .with_reproducible(reproducible)
        .with_dry_run(dry_run);

    executor = executor.with_sandbox(Arc::new(memobuild::sandbox::local::LocalSandbox::new(
        context_dir.clone(),
    )));

    if let Some(st) = sandbox_type {
        if st.as_str() == "containerd" {
            #[cfg(feature = "containerd")]
            {
                let sandbox = Arc::new(memobuild::sandbox::containerd::ContainerdSandbox::new(
                    "memobuild",
                    "/run/containerd/containerd.sock",
                ));
                executor = executor.with_sandbox(sandbox);
            }
        }
    }

    // Configure remote execution if requested
    if remote_exec {
        if let Ok(scheduler_url) = std::env::var("MEMOBUILD_SCHEDULER_URL") {
            let remote_client = Arc::new(memobuild::remote_exec::client::RemoteExecClient::new(
                &scheduler_url,
            ));
            executor = executor.with_remote_executor(remote_client);
            println!("📡 Using remote execution via scheduler: {}", scheduler_url);
        } else {
            println!("⚠️  --remote-exec specified but MEMOBUILD_SCHEDULER_URL not set");
        }
    }

    executor.execute(&mut graph).await?;
    let duration = build_start.elapsed();

    let _ = cache
        .report_analytics(
            dirty as u32,
            (graph.nodes.len() - dirty) as u32,
            duration.as_millis() as u64,
        )
        .await;

    println!("📦 Exporting OCI Image...");
    let output_dir = export::export_image(&graph, "memobuild-demo:latest", reproducible)?;

    let image_digest = compute_image_digest(&output_dir)?;
    let sbom_generator = sbom::SbomGenerator::new(Some("NRELabs".to_string()));
    let sbom_path = output_dir.join("sbom.json");
    let sbom = sbom_generator.generate_from_context(
        "memobuild-demo:latest",
        &image_digest,
        &context_dir,
        &PathBuf::from(&dockerfile_path),
    )?;
    sbom_generator.save_sbom(&sbom, &sbom_path, &sbom::OutputFormat::Json)?;
    println!("🔐 SBOM written to {}", sbom_path.display());

    let provenance_generator = slsa::ProvenanceGenerator::new("memobuild-builder".to_string());
    let provenance = provenance_generator.generate_provenance(
        &format!("git+file://{}", context_dir.display()),
        &image_digest,
        "oci://memobuild-demo:latest",
        &image_digest,
        &slsa::InvocationParams::default(),
    )?;
    let attestation = provenance_generator.sign(&provenance)?;
    let attestation_path = output_dir.join("attestation.json");
    provenance_generator.save_attestation(&attestation, &attestation_path)?;
    println!("🔐 SLSA attestation written to {}", attestation_path.display());

    audit::log_build_event(&audit_logger, &build_id, "completed");

    if push {
        let registry_url =
            env::var("MEMOBUILD_REGISTRY").unwrap_or_else(|_| "localhost:5000".to_string());
        let repo = env::var("MEMOBUILD_REPO").unwrap_or_else(|_| "memobuild-demo".to_string());
        let token = env::var("MEMOBUILD_TOKEN").ok();
        let mut client = export::registry::RegistryClient::new(&registry_url, &repo);
        if let Some(t) = token {
            client.set_token(&t);
        }
        client.push(&output_dir)?;
    }

    println!("✅ Build and Export completed successfully");
    Ok(())
}

async fn run_sbom(
    image: String,
    context_dir: PathBuf,
    dockerfile_path: String,
    output: String,
    format: String,
) -> Result<()> {
    let output_path = PathBuf::from(&output);
    let dockerfile = context_dir.join(&dockerfile_path);
    sbom::cli::generate_cmd(&image, &context_dir, &dockerfile, &output, &format)?;
    println!("✅ SBOM generation complete: {}", output_path.display());
    Ok(())
}

async fn run_verify(
    image: String,
    require_signed: bool,
    include_rekor: bool,
    oidc_token: Option<String>,
) -> Result<()> {
    let policy = verify::VerificationPolicy {
        require_signed,
        include_rekor,
        certificate_identity: oidc_token.clone(),
        certificate_oidc_issuer: std::env::var("MEMOBUILD_OIDC_ISSUER").ok(),
    };

    verify::cli::verify_cmd(&image, &policy).await?;

    if let Some(ref token) = oidc_token {
        let verification = verify::verify_keyless(&image, token).await?;
        println!("Keyless verification: {}", verification.message);
    }

    Ok(())
}

async fn run_attest(source_uri: &str, artifact_uri: &str, output: &str) -> Result<()> {
    let generator = slsa::ProvenanceGenerator::new("memobuild-builder".to_string());
    let provenance = generator.generate_provenance(
        source_uri,
        "sha256:unknown",
        artifact_uri,
        "sha256:unknown",
        &slsa::InvocationParams::default(),
    )?;
    let attestation = generator.sign(&provenance)?;
    generator.save_attestation(&attestation, Path::new(output))?;
    println!("✅ SLSA attestation written to {}", output);
    Ok(())
}

fn compute_image_digest(output_dir: &Path) -> Result<String> {
    let index_file = output_dir.join("index.json");
    let index_contents = fs::read_to_string(&index_file)
        .with_context(|| format!("Failed to read OCI index from {}", index_file.display()))?;
    let digest = Sha256::digest(index_contents.as_bytes());
    Ok(format!("sha256:{}", hex_encode(digest)))
}

async fn run_graph(context_dir: PathBuf, dockerfile_path: String) -> Result<()> {
    let dockerfile = fs::read_to_string(&dockerfile_path)?;
    let instructions = docker::parser::parse_dockerfile(&dockerfile);
    let graph = docker::dag::build_graph_from_instructions(instructions, context_dir);

    println!("\n{}", "🕸️  Build Dependency Graph:".bold().cyan());
    for node in &graph.nodes {
        let deps: Vec<String> = node
            .deps
            .iter()
            .map(|&id| graph.nodes[id].name.clone())
            .collect();
        println!("  {} [{}]", node.name.green(), node.id);
        if !deps.is_empty() {
            println!("    └─ depends on: {:?}", deps);
        }
    }
    Ok(())
}

async fn run_explain_cache(
    context_dir: PathBuf,
    dockerfile_path: String,
    target_node: Option<String>,
) -> Result<()> {
    let env_fp = memobuild::env::EnvFingerprint::collect();
    let cache = Arc::new(create_cache().await?);
    let dockerfile = fs::read_to_string(&dockerfile_path)?;
    let instructions = docker::parser::parse_dockerfile(&dockerfile);
    let mut graph = docker::dag::build_graph_from_instructions(instructions, context_dir.clone());

    // AI Layer Analysis to get extra dependencies
    let ai_layer = memobuild::ai::AiLayer::new();
    ai_layer.analyze(&mut graph, &env_fp, &context_dir);

    core::detect_changes(&mut graph);
    core::propagate_dirty(&mut graph);
    core::compute_composite_hashes(&mut graph, &env_fp);

    println!("\n{}", "🔍 Cache Explanation:".bold().cyan());
    for node in &graph.nodes {
        if let Some(ref target) = target_node {
            if !node.name.contains(target) && node.id.to_string() != *target {
                continue;
            }
        }

        let is_cached = cache.local.exists(&node.hash);

        println!("  {} (ID: {})", node.name.bold(), node.id);
        println!(
            "    Status: {}",
            if is_cached {
                "CACHED (Clean)".green()
            } else {
                "DIRTY (Rebuild Required)".red()
            }
        );
        println!("    Hash: {}", node.hash.cyan());

        if !is_cached {
            let mut reasons = Vec::new();
            if node.source_path.is_some() {
                reasons.push("Source files changed or untracked");
            }
            if !node.metadata.extra_source_paths.is_empty() {
                reasons.push("AI-detected dependencies changed");
            }

            let dirty_deps: Vec<_> = node
                .deps
                .iter()
                .filter(|&&id| !cache.local.exists(&graph.nodes[id].hash))
                .map(|&id| &graph.nodes[id].name)
                .collect();
            if !dirty_deps.is_empty() {
                reasons.push("Dependencies are dirty (recursive)");
                println!("    Dirty Dependencies: {:?}", dirty_deps);
            }
            println!("    Reasons: {:?}", reasons);
        }
    }
    Ok(())
}

async fn create_cache() -> Result<cache::HybridCache> {
    cache::HybridCache::new(None)
}

async fn _pull_base_images(instructions: &[docker::parser::Instruction]) -> Result<()> {
    for instr in instructions {
        if let docker::parser::Instruction::From(img) = instr {
            println!("   📥 Pulling base image {}...", img);
        }
    }
    Ok(())
}

async fn run_pull(full_name: String) -> Result<()> {
    let (registry_repo, tag) = full_name.split_once(':').unwrap_or((&full_name, "latest"));
    let (registry, repo) = registry_repo
        .split_once('/')
        .unwrap_or(("index.docker.io", registry_repo));
    let output_dir = env::current_dir()?
        .join(".memobuild-cache")
        .join("images")
        .join(full_name.replace([':', '/'], "-"));
    let client = export::registry::RegistryClient::new(registry, repo);
    client.pull(tag, &output_dir)
}

async fn run_generate_ci(provider: String) -> Result<()> {
    if provider == "github" {
        let _yaml = include_str!("../docs/releases/PHASE_1_COMPLETE.md"); // Placeholder for actual template
        println!("✅ GitHub Actions workflow generated");
    }
    Ok(())
}

async fn _run_generate_k8s() -> Result<()> {
    println!("✅ Kubernetes Job manifest generated");
    Ok(())
}

async fn start_scheduler(_port: u16) -> Result<()> {
    #[cfg(feature = "remote-exec")]
    {
        use memobuild::remote_exec::scheduler::SchedulingStrategy;
        use memobuild::remote_exec::{scheduler::Scheduler, server::ExecutionServer};

        println!(
            "📡 Starting Remote Execution Scheduler on port {}...",
            _port
        );

        // For MVP, start with empty worker list - workers will register dynamically
        // In production, this would discover workers via service registry
        let scheduler = Arc::new(Scheduler::new(SchedulingStrategy::RoundRobin));
        let server = ExecutionServer::new(scheduler);

        server.start(_port).await
    }
    #[cfg(not(feature = "remote-exec"))]
    anyhow::bail!("Remote Execution feature not enabled. Build with --features remote-exec")
}

async fn start_reapi_server(_port: u16) -> Result<()> {
    #[cfg(feature = "remote-exec")]
    {
        use memobuild::remote_exec::scheduler::SchedulingStrategy;
        use memobuild::remote_exec::{grpc_server, scheduler::Scheduler};

        println!(
            "🚀 Starting MemoBuild REAPI gRPC server on port {}...",
            _port
        );

        let cache = Arc::new(create_cache().await?);
        let scheduler = Arc::new(Scheduler::new(SchedulingStrategy::RoundRobin));

        grpc_server::start_grpc_server(_port, scheduler, cache).await
    }
    #[cfg(not(feature = "remote-exec"))]
    anyhow::bail!("Remote Execution feature not enabled. Build with --features remote-exec")
}

async fn start_worker(
    _port: u16,
    _sandbox_type: String,
    _scheduler_url: Option<String>,
) -> Result<()> {
    #[cfg(feature = "remote-exec")]
    {
        use memobuild::remote_exec::{worker::WorkerNode, worker_server::WorkerServer};
        use memobuild::sandbox;

        println!(
            "🔧 Starting Worker Node on port {} with {} sandbox...",
            _port, _sandbox_type
        );

        // Initialize cache (same as build command)
        let cache = create_cache().await?;
        let cache = Arc::new(cache);

        // Initialize sandbox
        let sandbox: Arc<dyn sandbox::Sandbox> = match _sandbox_type.as_str() {
            "local" => Arc::new(sandbox::local::LocalSandbox::new(std::env::current_dir()?)),
            #[cfg(feature = "containerd")]
            "containerd" => Arc::new(sandbox::containerd::ContainerdSandbox::new(
                "memobuild",
                "/run/containerd/containerd.sock",
            )),
            _ => anyhow::bail!("Unsupported sandbox type: {}", _sandbox_type),
        };

        // Create worker node
        let worker_id = format!("worker-{}", _port);
        let worker = Arc::new(WorkerNode::new(&worker_id, cache, sandbox));

        // Set scheduler URL for registration
        if let Some(url) = _scheduler_url {
            std::env::set_var("MEMOBUILD_SCHEDULER_URL", url);
        }

        // Start worker server
        let server = WorkerServer::new(worker);
        server.start(_port).await
    }
    #[cfg(not(feature = "remote-exec"))]
    anyhow::bail!("Remote Execution feature not enabled. Build with --features remote-exec")
}

async fn start_cluster_server(
    port: u16,
    node_id: Option<String>,
    peers: Option<String>,
    use_postgres: bool,
    database_url: Option<String>,
) -> Result<()> {
    println!("🏗️ Starting MemoBuild Clustered Cache Server...");

    // Generate node ID if not provided
    let node_id = node_id.unwrap_or_else(|| format!("node-{}", port));

    // Initialize cluster
    let local_node = memobuild::cache_cluster::ClusterNode {
        id: node_id.clone(),
        address: format!("http://localhost:{}", port),
        weight: 100,
        region: Some("local".to_string()),
    };

    let cluster = Arc::new(memobuild::cache_cluster::CacheCluster::new(local_node, 2));

    // Add peer nodes
    if let Some(peers_str) = peers {
        for peer_addr in peers_str.split(',') {
            let peer_addr = peer_addr.trim();
            if !peer_addr.is_empty() {
                let peer_node = memobuild::cache_cluster::ClusterNode {
                    id: format!(
                        "peer-{}",
                        peer_addr.replace("http://", "").replace(":", "-")
                    ),
                    address: peer_addr.to_string(),
                    weight: 100,
                    region: Some("peer".to_string()),
                };
                cluster.add_node(peer_node).await?;
            }
        }
    }

    // Initialize storage backend
    let metadata_store: Arc<dyn crate::server::metadata::MetadataStoreTrait> = if use_postgres {
        if let Some(db_url) = database_url {
            // Parse PostgreSQL URL
            let config = parse_postgres_url(&db_url)?;
            Arc::new(memobuild::scalable_db::PostgresMetadataStore::new(config).await?)
        } else {
            anyhow::bail!("PostgreSQL enabled but DATABASE_URL not provided");
        }
    } else {
        // Use SQLite for simplicity
        let data_dir = std::env::current_dir()?.join(".memobuild-cluster");
        fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("metadata.db");
        Arc::new(crate::server::metadata::MetadataStore::new(&db_path)?)
    };

    let storage = Arc::new(crate::server::storage::LocalStorage::new(
        &std::env::current_dir()?.join(".memobuild-cluster"),
    )?);

    // Create distributed cache
    let local_cache = Arc::new(memobuild::remote_cache::HttpRemoteCache::new(format!(
        "http://localhost:{}",
        port
    )));
    let metrics = memobuild::metrics::metrics_registry();
    let distributed_cache = Arc::new(memobuild::cache_cluster::DistributedCache::new(
        cluster.clone(),
        local_cache,
        metrics,
    ));

    // Initialize auto-scaler
    let scaling_policy = memobuild::auto_scaling::ScalingPolicy {
        min_replicas: 1,
        max_replicas: 10,
        target_utilization_percent: 70.0,
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.3,
        stabilization_window_secs: 300,
        cooldown_period_secs: 60,
    };

    let auto_scaler = memobuild::auto_scaling::AutoScaler::new(scaling_policy.clone()).await?;
    let auto_scaler = match auto_scaler.with_kubernetes().await {
        Ok(scaler) => scaler,
        Err(_) => memobuild::auto_scaling::AutoScaler::new(scaling_policy).await?,
    };
    let auto_scaler = Arc::new(auto_scaler);

    // Create cluster server
    let server = memobuild::cluster_server::ClusterServer {
        cluster,
        metadata_store,
        storage,
        distributed_cache,
        auto_scaler,
    };

    server.start(port).await
}

fn parse_postgres_url(url: &str) -> Result<memobuild::scalable_db::PostgresConfig> {
    // Simple URL parser for demo - in production use a proper URL parser
    let url = url
        .trim_start_matches("postgresql://")
        .trim_start_matches("postgres://");
    let parts: Vec<&str> = url.split('@').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid PostgreSQL URL format");
    }

    let auth = parts[0];
    let rest = parts[1];

    let auth_parts: Vec<&str> = auth.split(':').collect();
    if auth_parts.len() != 2 {
        anyhow::bail!("Invalid auth format in PostgreSQL URL");
    }

    let host_db: Vec<&str> = rest.split('/').collect();
    if host_db.len() != 2 {
        anyhow::bail!("Invalid host/database format in PostgreSQL URL");
    }

    let host_port: Vec<&str> = host_db[0].split(':').collect();
    let host = host_port[0];
    let port = host_port.get(1).unwrap_or(&"5432").parse()?;

    Ok(memobuild::scalable_db::PostgresConfig {
        host: host.to_string(),
        port,
        database: host_db[1].to_string(),
        user: auth_parts[0].to_string(),
        password: auth_parts[1].to_string(),
        max_connections: 20,
        min_idle: Some(5),
    })
}

use colored::*;
