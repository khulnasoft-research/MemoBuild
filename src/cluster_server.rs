//! Clustered Cache Server Implementation
//!
//! This module provides the HTTP server for the distributed cache cluster,
//! including cluster management endpoints and auto-scaling integration.

use crate::auto_scaling::{AutoScaler, ScalingMetrics};
use crate::cache_cluster::{CacheCluster, ClusterStatus};
use crate::remote_cache::RemoteCache;
use crate::server::metadata::MetadataStoreTrait;
use crate::server::storage::ArtifactStorage;
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Application state for the cluster server
pub struct ClusterAppState {
    pub cluster: Arc<CacheCluster>,
    pub metadata_store: Arc<dyn MetadataStoreTrait>,
    pub storage: Arc<dyn ArtifactStorage>,
    pub distributed_cache: Arc<dyn RemoteCache>,
    pub auto_scaler: Arc<AutoScaler>,
    pub tx_events: broadcast::Sender<crate::dashboard::BuildEvent>,
}

/// Cluster server with distributed cache and auto-scaling
pub struct ClusterServer {
    pub cluster: Arc<CacheCluster>,
    pub metadata_store: Arc<dyn MetadataStoreTrait>,
    pub storage: Arc<dyn ArtifactStorage>,
    pub distributed_cache: Arc<dyn RemoteCache>,
    pub auto_scaler: Arc<AutoScaler>,
}

impl ClusterServer {
    pub async fn start(self, port: u16) -> Result<()> {
        let (tx_events, _) = broadcast::channel(crate::constants::MAX_WS_BROADCAST_CAPACITY);

        let state = Arc::new(ClusterAppState {
            cluster: self.cluster,
            metadata_store: self.metadata_store,
            storage: self.storage,
            distributed_cache: self.distributed_cache,
            auto_scaler: self.auto_scaler,
            tx_events,
        });

        let app = Router::new()
            // Cache endpoints (same as regular server)
            .route("/cache/:hash", axum::routing::head(check_cache))
            .route("/cache/:hash", axum::routing::get(get_artifact))
            .route("/cache/:hash", axum::routing::put(put_artifact))
            .route("/cache/layer/:hash", axum::routing::head(check_layer))
            .route("/cache/layer/:hash", axum::routing::get(get_layer))
            .route("/cache/layer/:hash", axum::routing::put(put_layer))
            .route(
                "/cache/node/:hash/layers",
                axum::routing::get(get_node_layers),
            )
            .route(
                "/cache/node/:hash/layers",
                axum::routing::post(register_node_layers),
            )
            // Cluster management endpoints
            .route("/cluster/status", axum::routing::get(get_cluster_status))
            .route("/cluster/nodes", axum::routing::post(add_cluster_node))
            .route(
                "/cluster/nodes/:node_id",
                axum::routing::delete(remove_cluster_node),
            )
            // Auto-scaling endpoints
            .route("/scaling/status", axum::routing::get(get_scaling_status))
            .route(
                "/scaling/metrics",
                axum::routing::post(record_scaling_metrics),
            )
            .route("/scaling/predict", axum::routing::get(predict_resources))
            // Health check
            .route("/health", axum::routing::get(health_check))
            .with_state(state);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        println!("🏗️ MemoBuild Clustered Cache Server running on {}", addr);
        println!(
            "   📊 Cluster Status: http://localhost:{}/cluster/status",
            port
        );
        println!(
            "   ⚖️  Scaling Status: http://localhost:{}/scaling/status",
            port
        );

        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;

        Ok(())
    }
}

// Cluster management handlers
async fn get_cluster_status(
    State(state): State<Arc<ClusterAppState>>,
) -> Result<Json<ClusterStatus>, StatusCode> {
    match state.cluster.get_cluster_status().await {
        Ok(status) => Ok(Json(status)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
struct AddNodeRequest {
    id: String,
    address: String,
    weight: Option<u32>,
    region: Option<String>,
}

async fn add_cluster_node(
    State(state): State<Arc<ClusterAppState>>,
    Json(req): Json<AddNodeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let node = crate::cache_cluster::ClusterNode {
        id: req.id,
        address: req.address,
        weight: req.weight.unwrap_or(100),
        region: req.region,
    };

    match state.cluster.add_node(node).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "node_added"
        }))),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn remove_cluster_node(
    State(state): State<Arc<ClusterAppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.cluster.remove_node(&node_id).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "node_removed"
        }))),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// Auto-scaling handlers
async fn get_scaling_status(
    State(state): State<Arc<ClusterAppState>>,
) -> Result<Json<crate::auto_scaling::ScalingStatus>, StatusCode> {
    match state.auto_scaler.get_scaling_status().await {
        Ok(status) => Ok(Json(status)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn record_scaling_metrics(
    State(state): State<Arc<ClusterAppState>>,
    Json(metrics): Json<ScalingMetrics>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.auto_scaler.record_metrics(metrics).await {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "metrics_recorded"
        }))),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
struct PredictRequest {
    time_window_secs: Option<u64>,
}

async fn predict_resources(
    State(state): State<Arc<ClusterAppState>>,
    Query(params): Query<PredictRequest>,
) -> Result<Json<crate::auto_scaling::ResourcePrediction>, StatusCode> {
    let time_window = params.time_window_secs.unwrap_or(3600); // 1 hour default
    match state.auto_scaler.predict_resource_needs(time_window).await {
        Ok(prediction) => Ok(Json(prediction)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// Health check handler
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "memobuild-cluster",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// Cache handlers (reused from regular server)
async fn check_cache(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
) -> Result<StatusCode, StatusCode> {
    match state.distributed_cache.has(&hash).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_artifact(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    match state.distributed_cache.get(&hash).await {
        Ok(Some(data)) => Ok(data),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn put_artifact(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
    body: axum::body::Bytes,
) -> Result<StatusCode, StatusCode> {
    match state.distributed_cache.put(&hash, &body).await {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn check_layer(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
) -> Result<StatusCode, StatusCode> {
    match state.distributed_cache.has_layer(&hash).await {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_layer(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
) -> Result<Vec<u8>, StatusCode> {
    match state.distributed_cache.get_layer(&hash).await {
        Ok(Some(data)) => Ok(data),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn put_layer(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
    body: axum::body::Bytes,
) -> Result<StatusCode, StatusCode> {
    match state.distributed_cache.put_layer(&hash, &body).await {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_node_layers(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
) -> Result<Json<Option<Vec<String>>>, StatusCode> {
    match state.distributed_cache.get_node_layers(&hash).await {
        Ok(layers) => Ok(Json(layers)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(serde::Deserialize)]
struct RegisterLayersRequest {
    layers: Vec<String>,
    total_size: u64,
}

async fn register_node_layers(
    State(state): State<Arc<ClusterAppState>>,
    Path(hash): Path<String>,
    Json(req): Json<RegisterLayersRequest>,
) -> Result<StatusCode, StatusCode> {
    match state
        .distributed_cache
        .register_node_layers(&hash, &req.layers, req.total_size)
        .await
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
