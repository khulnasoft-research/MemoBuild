use memobuild::cache::HybridCache;
use memobuild::remote_exec::scheduler::{Scheduler, SchedulingStrategy};
use memobuild::remote_exec::worker::WorkerNode;
use memobuild::remote_exec::{ActionRequest, Digest, RemoteExecutor};
use memobuild::sandbox::local::LocalSandbox;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("🏗️  MemoBuild Build Farm Demo (Data Locality + Output Capture)");

    // 1. Setup Shared Cache
    let cache = Arc::new(HybridCache::new(None)?);
    let sandbox = Arc::new(LocalSandbox::new(std::env::current_dir()?));

    // 2. Spawn 3 Worker Nodes
    let mut workers: Vec<Arc<dyn RemoteExecutor>> = Vec::new();
    for i in 1..=3 {
        let worker = Arc::new(WorkerNode::new(
            &format!("worker-{}", i),
            cache.clone(),
            sandbox.clone(),
        ));
        workers.push(worker);
        println!("   ✅ Started Worker {}", i);
    }

    // 3. Initialize Scheduler with Data Locality
    let shared_scheduler = Arc::new(Scheduler::new(SchedulingStrategy::DataLocality));
    println!("   🚀 Scheduler running with DataLocality strategy\n");

    // 4. Dispatch tasks with some shared hashes to test Data Locality
    let mut handles = Vec::new();

    // Tasks 1 and 4 share the same input hash
    // Tasks 2 and 5 share another hash
    let hashes = vec![
        "shared-hash-A",
        "shared-hash-B",
        "unique-hash-C",
        "shared-hash-A",
        "shared-hash-B",
    ];

    for (i, hash) in hashes.into_iter().enumerate() {
        let sch = shared_scheduler.clone();
        let task_id = i + 1;
        let h = hash.to_string();

        let handle = tokio::spawn(async move {
            let action = ActionRequest {
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!(
                        "echo 'Content for {}' > output.txt; echo 'Task {} done'",
                        h, task_id
                    ),
                ],
                env: HashMap::new(),
                input_root_digest: Digest {
                    hash: h,
                    size_bytes: 1024,
                },
                timeout: Duration::from_secs(30),
                platform_properties: HashMap::new(),
                output_files: vec!["output.txt".to_string()],
                output_directories: Vec::new(),
            };

            sch.execute(action).await
        });
        handles.push(handle);
    }

    println!("📡 Dispatched 5 tasks. Expecting tasks with same hash to hit same worker...");

    for handle in handles {
        let result = handle.await??;
        println!(
            "   📥 Task completed: worker={}, outputs={:?}",
            result.execution_metadata.worker_id,
            result.output_files.keys().collect::<Vec<_>>()
        );
        if !result.stdout_raw.is_empty() {
            println!(
                "      Stdout: {}",
                String::from_utf8_lossy(&result.stdout_raw).trim()
            );
        }
    }

    println!("\n✨ build farm execution with data locality successful!");
    Ok(())
}
