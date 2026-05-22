//! Bazel Remote Execution API (REAPI) Implementation
//!
//! This module implements the google.devtools.remoteexecution.v2 services
//! to enable compatibility with Bazel and other REAPI clients.

use crate::cache::RemoteCache;
use crate::remote_exec::{
    ActionRequest, ActionResult, Digest, ExecutionMetadata, RemoteExecutor,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use chrono::Utc;
use sha2::Digest as Sha2Digest;
use serde_json;
use uuid::Uuid;

pub mod memobuild {
    pub mod v1 {
        tonic::include_proto!("memobuild.v1");
    }
}

use memobuild::v1::{
    execution_service_server::ExecutionService,
    cache_service_server::CacheService,
    ExecuteRequest, ExecuteResponse, WaitExecutionRequest,
    GetExecutionStreamInfoRequest, GetExecutionStreamInfoResponse,
    GetActionResultRequest, ActionResult as ProtoActionResult,
    UpdateActionResultRequest, UpdateActionResultResponse,
    FindMissingBlobsRequest, FindMissingBlobsResponse,
    BatchReadBlobsRequest, BatchReadBlobsResponse,
    BatchUpdateBlobsRequest, BatchUpdateBlobsResponse,
    GetTreeRequest, GetTreeResponse,
    Blob, BlobDigest, FileNode,
    action_result, execution_info,
};

/// REAPI-compatible Execution Service
pub struct ReapiExecutionService {
    scheduler: Arc<dyn RemoteExecutor>,
    cache: Arc<dyn RemoteCache>,
    active_executions: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl ReapiExecutionService {
    pub fn new(scheduler: Arc<dyn RemoteExecutor>, cache: Arc<dyn RemoteCache>) -> Self {
        Self {
            scheduler,
            cache,
            active_executions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn action_result_cache_key(action_digest: &str) -> String {
        format!("reapi-action-result/{}", action_digest)
    }

    async fn get_cached_action_result(
        &self,
        action_digest: &str,
    ) -> Result<Option<ActionResult>, Status> {
        match self.cache.get(&Self::action_result_cache_key(action_digest)).await {
            Ok(Some(payload)) => {
                serde_json::from_slice::<ActionResult>(&payload)
                    .map(Some)
                    .map_err(|e| Status::internal(format!("Failed to deserialize cached action result: {}", e)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Status::internal(format!("Cache error: {}", e))),
        }
    }

    async fn put_cached_action_result(&self, action_digest: &str, result: &ActionResult) -> Result<(), Status> {
        let payload = serde_json::to_vec(result)
            .map_err(|e| Status::internal(format!("Failed to serialize action result: {}", e)))?;

        self.cache
            .put(&Self::action_result_cache_key(action_digest), &payload)
            .await
            .map_err(|e| Status::internal(format!("Cache error: {}", e)))
    }
}

#[tonic::async_trait]
impl ExecutionService for ReapiExecutionService {
    type ExecuteStream = tokio_stream::wrappers::ReceiverStream<Result<ExecuteResponse, Status>>;

    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();

        // Parse action digest
        let action_digest_parts: Vec<&str> = req.action_digest.split('/').collect();
        if action_digest_parts.len() != 2 {
            return Err(Status::invalid_argument("Invalid action digest format"));
        }

        let action_digest = Digest {
            hash: action_digest_parts[0].to_string(),
            size_bytes: action_digest_parts[1].parse().map_err(|_| Status::invalid_argument("Invalid digest size"))?,
        };

        // Check cached action result first
        if let Ok(Some(cached_result)) = self.get_cached_action_result(&action_digest.hash).await {
            let response = ExecuteResponse {
                output: Some(execute_response::Output::ExitCode(
                    execute_response::ExitCode {
                        exit_code: cached_result.exit_code,
                    }
                )),
            };
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.send(Ok(response)).await.unwrap();
            return Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)));
        }

        // Create execution request
        let action_request = ActionRequest {
            command: vec!["/bin/sh".to_string(), "-c".to_string(), "echo 'placeholder'".to_string()], // TODO: Parse from action
            env: HashMap::new(),
            input_root_digest: action_digest.clone(),
            timeout: std::time::Duration::from_secs(300),
            platform_properties: HashMap::new(),
            output_files: vec![],
            output_directories: vec![],
        };

        // Schedule execution
        let execution_id = format!("exec-{}", Uuid::new_v4());
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // Spawn execution task
        let scheduler = self.scheduler.clone();
        let cache = self.cache.clone();
        let active_executions = self.active_executions.clone();
        let execution_id_clone = execution_id.clone();

        let handle = tokio::spawn(async move {
            // Send execution started
            let _ = tx.send(Ok(ExecuteResponse {
                output: Some(execute_response::Output::ExecutionMetadata(
                    execute_response::ExecutionMetadata {
                        worker: "worker-1".to_string(),
                        queued_duration_ns: 0,
                        worker_start_timestamp_ns: Utc::now().timestamp_nanos(),
                        worker_completed_timestamp_ns: 0,
                        input_fetch_duration_ns: 0,
                        output_upload_duration_ns: 0,
                    }
                )),
            })).await;

            // Execute action
            match scheduler.execute(action_request).await {
                Ok(result) => {
                    // Send stdout/stderr
                    if !result.stdout_raw.is_empty() {
                        let _ = tx.send(Ok(ExecuteResponse {
                            output: Some(execute_response::Output::Stdout(
                                execute_response::Stdout {
                                    data: result.stdout_raw.clone(),
                                }
                            )),
                        })).await;
                    }

                    if !result.stderr_raw.is_empty() {
                        let _ = tx.send(Ok(ExecuteResponse {
                            output: Some(execute_response::Output::Stderr(
                                execute_response::Stderr {
                                    data: result.stderr_raw.clone(),
                                }
                            )),
                        })).await;
                    }

                    // Send exit code
                    let _ = tx.send(Ok(ExecuteResponse {
                        output: Some(execute_response::Output::ExitCode(
                            execute_response::ExitCode {
                                exit_code: result.exit_code,
                            }
                        )),
                    })).await;

                    // Cache result
                    let _ = cache.put(&ReapiExecutionService::action_result_cache_key(&action_digest.hash), &serde_json::to_vec(&result).unwrap_or_default()).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(format!("Execution failed: {}", e)))).await;
                }
            }

            let _ = active_executions.write().await.remove(&execution_id_clone);
        });

        // Store handle
        {
            let mut executions = self.active_executions.write().await;
            executions.insert(execution_id, handle);
        }

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn wait_execution(
        &self,
        _request: Request<WaitExecutionRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        // TODO: Implement reconnect support
        Err(Status::unimplemented("WaitExecution not implemented"))
    }

    async fn get_execution_stream_info(
        &self,
        request: Request<GetExecutionStreamInfoRequest>,
    ) -> Result<Response<GetExecutionStreamInfoResponse>, Status> {
        let operation_name = request.into_inner().operation_name;

        let executions = self.active_executions.read().await;
        let done = !executions.contains_key(&operation_name);

        Ok(Response::new(GetExecutionStreamInfoResponse {
            operation_name,
            done,
            error: String::new(),
            result: None, // TODO: Return cached result if done
        }))
    }
}

/// REAPI-compatible Cache Service
pub struct ReapiCacheService {
    cache: Arc<dyn RemoteCache>,
}

impl ReapiCacheService {
    pub fn new(cache: Arc<dyn RemoteCache>) -> Self {
        Self { cache }
    }
}

#[tonic::async_trait]
impl CacheService for ReapiCacheService {
    async fn get_action_result(
        &self,
        request: Request<GetActionResultRequest>,
    ) -> Result<Response<ProtoActionResult>, Status> {
        let action_digest = request.into_inner().action_digest;

        match self.get_cached_action_result(&action_digest).await {
            Ok(Some(result)) => {
                let stdout_digest = Sha2Digest::digest(&result.stdout_raw);
                let stderr_digest = Sha2Digest::digest(&result.stderr_raw);
                let proto_result = ProtoActionResult {
                    action_digest: action_digest.clone(),
                    exit_code: result.exit_code,
                    stdout: Some(action_result::StdoutFile {
                        digest: format!("sha256:{}", hex::encode(stdout_digest)),
                        size: result.stdout_raw.len() as i64,
                    }),
                    stderr: Some(action_result::StderrFile {
                        digest: format!("sha256:{}", hex::encode(stderr_digest)),
                        size: result.stderr_raw.len() as i64,
                    }),
                    execution_info: Some(execution_info::ExecutionInfo {
                        input_fetch_completed_timestamp: result.execution_metadata.worker_start_timestamp.unwrap_or(0),
                        execution_completed_timestamp: result.execution_metadata.worker_completed_timestamp.unwrap_or(0),
                        execution_worker: result.execution_metadata.worker_id,
                    }),
                    message_cache_info: None,
                };

                Ok(Response::new(proto_result))
            }
            Ok(None) => Err(Status::not_found("Action result not found")),
            Err(e) => Err(Status::internal(format!("Cache error: {}", e))),
        }
    }

    async fn update_action_result(
        &self,
        request: Request<UpdateActionResultRequest>,
    ) -> Result<Response<UpdateActionResultResponse>, Status> {
        let proto_result = request.into_inner().action_result;

        let result = ActionResult {
            output_files: HashMap::new(),
            exit_code: proto_result.exit_code,
            stdout_raw: Vec::new(),
            stderr_raw: Vec::new(),
            execution_metadata: ExecutionMetadata {
                worker_id: proto_result.execution_info.as_ref().map(|ei| ei.execution_worker.clone()).unwrap_or_default(),
                queued_timestamp: None,
                worker_start_timestamp: proto_result.execution_info.as_ref().map(|ei| Some(ei.input_fetch_completed_timestamp)),
                worker_completed_timestamp: proto_result.execution_info.as_ref().map(|ei| Some(ei.execution_completed_timestamp)),
            },
        };

        self.put_cached_action_result(&proto_result.action_digest, &result)
            .await
            .map_err(|e| Status::internal(format!("Cache error: {}", e)))?;

        Ok(Response::new(UpdateActionResultResponse { ok: true }))
    }

    async fn find_missing_blobs(
        &self,
        request: Request<FindMissingBlobsRequest>,
    ) -> Result<Response<FindMissingBlobsResponse>, Status> {
        let blob_digests = request.into_inner().blob_digests;

        let mut missing = Vec::new();
        for digest in blob_digests {
            if !self.cache.has(&digest).await.map_err(|e| Status::internal(format!("Cache error: {}", e)))? {
                missing.push(digest);
            }
        }

        Ok(Response::new(FindMissingBlobsResponse {
            missing_blob_digests: missing,
        }))
    }

    type BatchReadBlobsStream = tokio_stream::wrappers::ReceiverStream<Result<BatchReadBlobsResponse, Status>>;

    async fn batch_read_blobs(
        &self,
        request: Request<BatchReadBlobsRequest>,
    ) -> Result<Response<Self::BatchReadBlobsStream>, Status> {
        let blob_digests = request.into_inner().blob_digests;
        let (tx, rx) = tokio::sync::mpsc::channel(blob_digests.len());

        for digest in blob_digests {
            let cache = self.cache.clone();
            let tx = tx.clone();

            tokio::spawn(async move {
                match cache.get(&digest).await {
                    Ok(Some(data)) => {
                        let _ = tx.send(Ok(BatchReadBlobsResponse {
                            blob: Some(Blob {
                                digest: digest.clone(),
                                data,
                            }),
                        })).await;
                    }
                    Ok(None) => {
                        let _ = tx.send(Err(Status::not_found(format!("Blob {} not found", digest)))).await;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(Status::internal(format!("Cache error: {}", e)))).await;
                    }
                }
            });
        }

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn batch_update_blobs(
        &self,
        request: Request<BatchUpdateBlobsRequest>,
    ) -> Result<Response<BatchUpdateBlobsResponse>, Status> {
        let blobs = request.into_inner().blobs;

        let mut blob_digests = Vec::new();
        for blob in blobs {
            let already_cached = self.cache.has(&blob.digest).await
                .map_err(|e| Status::internal(format!("Cache error: {}", e)))?;

            if !already_cached {
                self.cache.put(&blob.digest, &blob.data).await
                    .map_err(|e| Status::internal(format!("Cache error: {}", e)))?;
            }

            blob_digests.push(BlobDigest {
                digest: blob.digest,
                already_cached,
            });
        }

        Ok(Response::new(BatchUpdateBlobsResponse { blob_digests }))
    }

    async fn get_tree(
        &self,
        _request: Request<GetTreeRequest>,
    ) -> Result<Response<GetTreeResponse>, Status> {
        // TODO: Implement directory tree traversal
        Err(Status::unimplemented("GetTree not implemented"))
    }
}