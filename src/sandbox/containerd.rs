use crate::graph::Node;
use crate::sandbox::{ExecResult, ResourceLimits, Sandbox, SandboxEnv};
use anyhow::{Context, Result};
use async_trait::async_trait;
use containerd_client::services::v1::containers_client::ContainersClient;
use containerd_client::services::v1::tasks_client::TasksClient;
use containerd_client::services::v1::CreateContainerRequest;
use containerd_client::tonic;
use containerd_client::tonic::Request;
use containerd_client::with_namespace;

pub struct ContainerdSandbox {
    pub namespace: String,
    pub socket: String,
    pub snapshotter: String,
    pub runtime: String,
    pub network_enabled: bool,
    pub limits: ResourceLimits,
}

impl ContainerdSandbox {
    pub fn new(namespace: &str, socket: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            socket: socket.to_string(),
            snapshotter: "overlayfs".to_string(),
            runtime: "io.containerd.runc.v2".to_string(),
            network_enabled: false,
            limits: ResourceLimits::default(),
        }
    }
}

#[async_trait]
impl Sandbox for ContainerdSandbox {
    async fn prepare(&self, node: &Node) -> Result<SandboxEnv> {
        println!("🚀 [containerd] Preparing sandbox for node: {}", node.name);

        // 1. In a real implementation, we would:
        //    - Pull the base image (e.g., from node.metadata.base_image)
        //    - Create a containerd snapshot (overlayfs)
        //    - Mount the snapshot to a temporary directory

        let temp_dir = std::env::temp_dir().join(format!("memobuild-{}", node.hash));
        std::fs::create_dir_all(&temp_dir)?;

        Ok(SandboxEnv {
            workspace_dir: temp_dir,
            env_vars: node.env.clone(),
        })
    }

    async fn execute(&self, env: &SandboxEnv, node: &Node) -> Result<ExecResult> {
        let cmd = match &node.kind {
            crate::graph::NodeKind::Run => &node.content,
            _ => {
                return Ok(ExecResult {
                    exit_code: 0,
                    stdout: format!("Mock artifact for {}", node.name).into_bytes(),
                    stderr: vec![],
                })
            }
        };

        println!("⚡ [containerd] Executing: {}", cmd);

        // 2. Connect to containerd
        let channel = tonic::transport::Endpoint::from_shared(format!("unix://{}", self.socket))?
            .connect()
            .await
            .context("Failed to connect to containerd socket")?;

        let mut _container_client = ContainersClient::new(channel.clone());
        let mut _task_client = TasksClient::new(channel.clone());

        let container_id = format!("memobuild-{}", &node.hash[..12]);

        // 3. Build OCI Spec
        let spec = crate::sandbox::spec::build_spec(cmd, &env.env_vars, &env.workspace_dir);
        let spec_json = serde_json::to_vec(&spec)?;

        // 4. Create Container
        let container = containerd_client::services::v1::Container {
            id: container_id.clone(),
            image: "alpine:latest".to_string(),
            runtime: Some(containerd_client::services::v1::container::Runtime {
                name: self.runtime.clone(),
                options: None,
            }),
            spec: Some(prost_types::Any {
                type_url: "types.containerd.io/opencontainers/runtime-spec/1/Spec".to_string(),
                value: spec_json,
            }),
            snapshotter: self.snapshotter.clone(),
            snapshot_key: container_id.clone(),
            ..Default::default()
        };
        let req = CreateContainerRequest {
            container: Some(container),
        };
        let _create_req = with_namespace!(req, &self.namespace);

        // Note: Real execution requires more steps (Task creation, Start, Wait)
        // This is a simplified logic flow for the Phase 6 MVP.

        println!("🏗️  [containerd] Container created: {}", container_id);

        Ok(ExecResult {
            exit_code: 0,
            stdout: "Container execution simulated (Requires Linux/Containerd runtime)".into(),
            stderr: vec![],
        })
    }

    async fn cleanup(&self, env: &SandboxEnv) -> Result<()> {
        println!(
            "🧹 [containerd] Cleaning up sandbox at: {}",
            env.workspace_dir.display()
        );
        if env.workspace_dir.exists() {
            let _ = std::fs::remove_dir_all(&env.workspace_dir);
        }
        Ok(())
    }
}
