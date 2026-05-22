use crate::remote_exec::{scheduler::Scheduler, ActionRequest, ActionResult};
use anyhow::Result;
use axum::{
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(serde::Deserialize)]
struct WorkerRegistration {
    worker_id: String,
    endpoint: String,
}

pub struct ExecutionServer {
    scheduler: Arc<Scheduler>,
}

impl ExecutionServer {
    pub fn new(scheduler: Arc<Scheduler>) -> Self {
        Self { scheduler }
    }

    pub async fn start(self, port: u16) -> Result<()> {
        let app = Router::new()
            .route("/execute", post(handle_execute))
            .route("/workers/register", post(handle_register_worker))
            .route("/workers", get(handle_list_workers))
            .layer(Extension(self.scheduler.clone()));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        println!("🚀 Remote Execution Scheduler listening on {}", addr);

        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))
    }
}

async fn handle_execute(
    Extension(scheduler): Extension<Arc<Scheduler>>,
    Json(action): Json<ActionRequest>,
) -> Result<Json<ActionResult>, (StatusCode, String)> {
    use crate::remote_exec::RemoteExecutor;

    match scheduler.execute(action).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn handle_register_worker(
    Extension(scheduler): Extension<Arc<Scheduler>>,
    Json(registration): Json<WorkerRegistration>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    scheduler
        .register_worker(registration.worker_id.clone(), registration.endpoint)
        .await;

    Ok(Json(serde_json::json!({
        "status": "registered",
        "worker_id": registration.worker_id
    })))
}

async fn handle_list_workers(Extension(scheduler): Extension<Arc<Scheduler>>) -> Json<Vec<String>> {
    let workers = scheduler.get_available_workers().await;
    let worker_ids: Vec<String> = workers.into_iter().map(|(id, _)| id).collect();
    Json(worker_ids)
}
