use memobuild::core;
use memobuild::docker::dag::build_graph_from_instructions;
use memobuild::docker::parser::parse_dockerfile;
use memobuild::export::export_image;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn test_reproducible_exports_are_identical() {
    let _ = tracing_subscriber::fmt::try_init();

    let dockerfile = "FROM scratch\nENV FOO=bar\nWORKDIR /app";
    let instructions = parse_dockerfile(dockerfile);

    // Build #1
    let cache_dir_1 = tempdir().unwrap();
    std::env::set_var("MEMOBUILD_CACHE_DIR", cache_dir_1.path());
    let cache_1 = Arc::new(memobuild::cache::HybridCache::new(None).unwrap());

    let env_fp = memobuild::env::EnvFingerprint::collect();
    let mut graph_1 =
        build_graph_from_instructions(instructions.clone(), std::env::current_dir().unwrap());

    core::detect_changes(&mut graph_1);
    core::propagate_dirty(&mut graph_1);
    core::compute_composite_hashes(&mut graph_1, &env_fp);
    core::propagate_manifests(&mut graph_1);

    let mut executor_1 =
        memobuild::executor::IncrementalExecutor::new(cache_1).with_reproducible(true);

    executor_1.execute(&mut graph_1).await.unwrap();

    let out_path_1 = export_image(&graph_1, "test-repro:v1", true).unwrap();
    let digest_1 = fs::read_to_string(out_path_1.join("index.json")).unwrap();

    // Sleep a bit to ensure timestamps would differ if not fixed
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Build #2
    let cache_dir_2 = tempdir().unwrap();
    std::env::set_var("MEMOBUILD_CACHE_DIR", cache_dir_2.path());
    let cache_2 = Arc::new(memobuild::cache::HybridCache::new(None).unwrap());

    let mut graph_2 = build_graph_from_instructions(instructions, std::env::current_dir().unwrap());

    core::detect_changes(&mut graph_2);
    core::propagate_dirty(&mut graph_2);
    core::compute_composite_hashes(&mut graph_2, &env_fp);
    core::propagate_manifests(&mut graph_2);

    let mut executor_2 =
        memobuild::executor::IncrementalExecutor::new(cache_2).with_reproducible(true);

    executor_2.execute(&mut graph_2).await.unwrap();

    let out_path_2 = export_image(&graph_2, "test-repro:v2", true).unwrap();
    let digest_2 = fs::read_to_string(out_path_2.join("index.json")).unwrap();

    // The two output registries must exactly match
    assert_eq!(
        digest_1, digest_2,
        "Reproducible builds should produce identical index.json and digests"
    );
}
