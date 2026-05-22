use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

pub mod client;
pub mod scheduler;
#[cfg(feature = "remote-exec")]
pub mod grpc_server;
#[cfg(feature = "remote-exec")]
pub mod reapi;
#[cfg(any(feature = "server", feature = "remote-exec"))]
pub mod server;
pub mod worker;
#[cfg(any(feature = "server", feature = "remote-exec"))]
pub mod worker_server;

/// Digest follows the Bazel / REAPI (Remote Execution API) specification.
/// In REAPI, objects are identified by their content hash and size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Digest {
    pub hash: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub command: Vec<String>,
    pub env: HashMap<String, String>,
    pub input_root_digest: Digest,
    pub timeout: Duration,
    pub platform_properties: HashMap<String, String>,
    pub output_files: Vec<String>,
    pub output_directories: Vec<String>,
}

/// ActionResult represents the result of a remote execution.
/// Maps to: google.devtools.remoteexecution.v2.ActionResult
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub output_files: HashMap<String, Digest>,
    pub exit_code: i32,
    pub stdout_raw: Vec<u8>,
    pub stderr_raw: Vec<u8>,
    pub execution_metadata: ExecutionMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionMetadata {
    pub worker_id: String,
    pub queued_timestamp: Option<i64>,
    pub worker_start_timestamp: Option<i64>,
    pub worker_completed_timestamp: Option<i64>,
}

#[async_trait]
pub trait RemoteExecutor: Send + Sync {
    async fn execute(&self, action: ActionRequest) -> Result<ActionResult>;
}
