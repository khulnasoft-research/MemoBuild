//! gRPC Server for Remote Execution API
//!
//! This module implements the gRPC services defined in the proto file.

use crate::cache::RemoteCache;
use crate::remote_exec::RemoteExecutor;
use crate::remote_exec::reapi::memobuild;
use reapi::{ReapiExecutionService, ReapiCacheService};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;

pub async fn start_grpc_server(
    port: u16,
    scheduler: Arc<dyn RemoteExecutor>,
    cache: Arc<dyn RemoteCache>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("Starting gRPC server on {}", addr);

    // Create REAPI services
    let execution_service = ReapiExecutionService::new(scheduler, cache.clone());
    let cache_service = ReapiCacheService::new(cache);

    Server::builder()
        .add_service(
            memobuild::v1::execution_service_server::ExecutionServiceServer::new(execution_service)
        )
        .add_service(
            memobuild::v1::cache_service_server::CacheServiceServer::new(cache_service)
        )
        .add_service(
            tonic_reflection::server::Builder::new()
                .register_encoded_file_descriptor_set(memobuild::v1::FILE_DESCRIPTOR_SET)
                .build()
                .unwrap()
        )
        .serve(addr)
        .await?;

    Ok(())
}