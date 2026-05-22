use crate::remote_exec::{worker::WorkerNode, ActionRequest, ActionResult};
use anyhow::Result;
use axum::{http::StatusCode, routing::post, Extension, Json, Router};
use reqwest::Client;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(serde::Serialize)]
struct WorkerRegistration {
    worker_id: String,
    endpoint: String,
}

pub struct WorkerServer {
    worker: Arc<WorkerNode>,
    scheduler_endpoint: Option<String>,
}

impl WorkerServer {
    pub fn new(worker: Arc<WorkerNode>) -> Self {
        Self {
            worker,
            scheduler_endpoint: std::env::var("MEMOBUILD_SCHEDULER_URL").ok(),
        }
    }

    pub async fn start(self, port: u16) -> Result<()> {
        let worker_id = self.worker.id.clone();

        // Register with scheduler if configured
        if let Some(scheduler_url) = &self.scheduler_endpoint {
            self.register_with_scheduler(scheduler_url, port).await?;
        }

        let app = Router::new()
            .route("/execute", post(handle_worker_execute))
            .layer(Extension(self.worker));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        println!("👷 Worker Node {} listening on {}", worker_id, addr);

        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .map_err(|e| anyhow::anyhow!("Worker server error: {}", e))
    }

    async fn register_with_scheduler(&self, scheduler_url: &str, port: u16) -> Result<()> {
        let client = Client::new();
        let registration = WorkerRegistration {
            worker_id: self.worker.id.clone(),
            endpoint: format!("http://localhost:{}", port),
        };

        let url = format!("{}/workers/register", scheduler_url.trim_end_matches('/'));
        println!(
            "📡 Registering worker {} with scheduler at {}",
            self.worker.id, url
        );

        let response = client
            .post(&url)
            .json(&registration)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to register with scheduler: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Scheduler registration failed: {} - {}", status, err_text);
        }

        println!(
            "✅ Worker {} successfully registered with scheduler",
            self.worker.id
        );
        Ok(())
    }
}

async fn handle_worker_execute(
    Extension(worker): Extension<Arc<WorkerNode>>,
    Json(action): Json<ActionRequest>,
) -> Result<Json<ActionResult>, (StatusCode, String)> {
    use crate::remote_exec::RemoteExecutor;

    match worker.execute(action).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
