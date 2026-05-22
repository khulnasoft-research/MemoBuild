#[cfg(feature = "server")]
use memobuild::{cache, core, executor, remote_cache, server};
use memobuild::{docker, graph::NodeKind};
#[cfg(feature = "server")]
use std::fs;
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use std::time::Duration;
#[cfg(feature = "server")]
use tempfile::tempdir;

#[test]
fn test_dag_linking() {
    // Test that COPY package.json and RUN npm install have correct dependency relationship
    let dockerfile_content = r#"
FROM node:16-alpine
COPY package.json .
RUN npm install
COPY . .
RUN npm run build
"#;

    let instructions = docker::parser::parse_dockerfile(dockerfile_content);
    let graph = docker::dag::build_graph_from_instructions(
        instructions,
        std::env::current_dir().unwrap_or_default(),
    );

    // Verify we have 5 nodes
    assert_eq!(graph.nodes.len(), 5, "Should have 5 nodes");

    // Find COPY package.json node (should be node 2)
    let copy_package_idx = graph.nodes.iter()
        .position(|n| matches!(&n.kind, NodeKind::Copy { src, .. } if src.to_string_lossy() == "package.json"))
        .expect("Should find COPY package.json node");

    // Find RUN npm install node (should be node 3)
    let run_npm_idx = graph
        .nodes
        .iter()
        .position(|n| matches!(&n.kind, NodeKind::Run) && n.content.contains("npm install"))
        .expect("Should find RUN npm install node");

    // RUN npm install should depend on COPY package.json
    assert!(
        graph.nodes[run_npm_idx].deps.contains(&copy_package_idx),
        "RUN npm install should depend on COPY package.json"
    );

    println!("✅ DAG linking test passed: RUN npm install correctly depends on COPY package.json");
}

#[test]
fn test_parallel_levels() {
    // Test that independent nodes are grouped into the same execution level
    let dockerfile_content = r#"
FROM node:16-alpine
ENV NODE_ENV=production
WORKDIR /app
COPY package.json .
COPY package-lock.json .
RUN npm install --only=production
"#;

    let instructions = docker::parser::parse_dockerfile(dockerfile_content);
    let graph = docker::dag::build_graph_from_instructions(
        instructions,
        std::env::current_dir().unwrap_or_default(),
    );

    // Get execution levels
    let levels = graph.levels();

    // Should have multiple levels
    assert!(levels.len() > 1, "Should have multiple execution levels");

    // Level 0 should contain FROM node (no dependencies)
    assert_eq!(levels[0].len(), 1, "Level 0 should have 1 node (FROM)");

    // Find ENV and WORKDIR nodes - they should be in the same level if independent
    let env_idx = graph
        .nodes
        .iter()
        .position(|n| matches!(&n.kind, NodeKind::Env))
        .expect("Should find ENV node");

    let workdir_idx = graph
        .nodes
        .iter()
        .position(|n| matches!(&n.kind, NodeKind::Workdir))
        .expect("Should find WORKDIR node");

    // ENV and WORKDIR should be parallelizable
    assert!(
        graph.nodes[env_idx].metadata.parallelizable,
        "ENV node should be parallelizable"
    );
    assert!(
        graph.nodes[workdir_idx].metadata.parallelizable,
        "WORKDIR node should be parallelizable"
    );

    // Find COPY nodes - they should be parallelizable with each other if they don't conflict
    let copy_package_idx = graph.nodes.iter()
        .position(|n| matches!(&n.kind, NodeKind::Copy { src, .. } if src.to_string_lossy() == "package.json"))
        .expect("Should find COPY package.json node");

    let copy_lock_idx = graph.nodes.iter()
        .position(|n| matches!(&n.kind, NodeKind::Copy { src, .. } if src.to_string_lossy() == "package-lock.json"))
        .expect("Should find COPY package-lock.json node");

    assert!(
        graph.nodes[copy_package_idx].metadata.parallelizable,
        "COPY package.json should be parallelizable"
    );
    assert!(
        graph.nodes[copy_lock_idx].metadata.parallelizable,
        "COPY package-lock.json should be parallelizable"
    );

    println!(
        "✅ Parallel levels test passed: {} levels with proper parallelizable nodes",
        levels.len()
    );
}

#[test]
fn test_content_addressed_identities() {
    // Test that compute_node_key produces consistent, content-addressed hashes
    let dockerfile_content = r#"
FROM node:16-alpine
COPY package.json .
RUN npm install
"#;

    let instructions = docker::parser::parse_dockerfile(dockerfile_content);
    let graph = docker::dag::build_graph_from_instructions(
        instructions,
        std::env::current_dir().unwrap_or_default(),
    );

    // Compute node keys
    let dep_hashes: Vec<String> = vec![]; // No dependencies for FROM node
    let from_key1 = graph.nodes[0].compute_node_key(&dep_hashes, None, None);
    let from_key2 = graph.nodes[0].compute_node_key(&dep_hashes, None, None);

    // Same node should produce same key
    assert_eq!(from_key1, from_key2, "Same node should produce same key");

    // Different nodes should produce different keys
    let copy_key = graph.nodes[1].compute_node_key(std::slice::from_ref(&from_key1), None, None);
    assert_ne!(
        from_key1, copy_key,
        "Different nodes should produce different keys"
    );

    // Key should change with different context hash
    let copy_key_with_context =
        graph.nodes[1].compute_node_key(&[from_key1], Some("different_context"), None);
    assert_ne!(
        copy_key, copy_key_with_context,
        "Key should change with different context"
    );

    println!(
        "✅ Content-addressed identities test passed: Node keys are consistent and context-aware"
    );
}

#[cfg(feature = "server")]
#[tokio::test]
async fn test_end_to_end_build_with_remote_cache() {
    // 1. Setup temporary directories
    let server_dir = tempdir().expect("Failed to create server temp dir");
    let client_dir = tempdir().expect("Failed to create client temp dir");
    let build_dir = tempdir().expect("Failed to create build temp dir");

    let server_path = server_dir.path().to_path_buf();
    let client_path = client_dir.path().to_path_buf();
    let build_path = build_dir.path().to_path_buf();

    // 2. Start Remote Cache Server in background task
    let port = 9991;
    let server_path_clone = server_path.clone();
    tokio::spawn(async move {
        server::start_server(port, server_path_clone, None, None, None, None)
            .await
            .ok();
    });

    // Wait for server to start
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // 3. Configure Client Environment
    std::env::set_var("MEMOBUILD_CACHE_DIR", client_path.to_str().unwrap());

    // 4. Create a dummy project in build_dir
    let dockerfile_path = build_path.join("Dockerfile");
    fs::write(
        &dockerfile_path,
        "FROM alpine\nRUN echo 'hello world' > hello.txt",
    )
    .unwrap();

    let original_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&build_path).unwrap();

    // 5. Initialize Cache with Remote Support
    let remote_url = format!("http://127.0.0.1:{}", port);
    let remote = Arc::new(remote_cache::HttpRemoteCache::new(remote_url));
    let cache = Arc::new(
        cache::HybridCache::new(Some(remote as Arc<dyn remote_cache::RemoteCache>)).unwrap(),
    );

    // 6. Run First Build (Populate Cache)
    let dockerfile_content = "FROM alpine\nRUN echo 'hello world' > hello.txt";
    let instructions = docker::parser::parse_dockerfile(dockerfile_content);
    let mut graph = docker::dag::build_graph_from_instructions(
        instructions,
        std::env::current_dir().unwrap_or_default(),
    );

    core::detect_changes(&mut graph);
    core::propagate_dirty(&mut graph);

    executor::execute_graph(&mut graph, cache.clone(), None, false)
        .await
        .expect("First build failed");

    // 7. Run Second Build (Should be cached)
    // Manually clear local cache to force remote fetch
    fs::remove_dir_all(&client_path).unwrap();
    fs::create_dir_all(&client_path).unwrap();

    let instructions2 = docker::parser::parse_dockerfile(dockerfile_content);
    let mut graph2 = docker::dag::build_graph_from_instructions(
        instructions2,
        std::env::current_dir().unwrap_or_default(),
    );

    core::detect_changes(&mut graph2);
    core::propagate_dirty(&mut graph2);

    let remote2 = Arc::new(remote_cache::HttpRemoteCache::new(format!(
        "http://127.0.0.1:{}",
        port
    )));
    let cache2 = Arc::new(
        cache::HybridCache::new(Some(remote2 as Arc<dyn remote_cache::RemoteCache>)).unwrap(),
    );

    executor::execute_graph(&mut graph2, cache2.clone(), None, false)
        .await
        .expect("Second build failed");

    // Verify that nodes were fetched from remote (cached should be > 0)
    let _cached_count = graph2.nodes.iter().filter(|n| !n.dirty).count();
    assert!(
        _cached_count > 0,
        "Build should have used remote cache items"
    );

    // 8. Cleanup
    std::env::set_current_dir(original_cwd).unwrap();
}
