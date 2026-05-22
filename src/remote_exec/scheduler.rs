use crate::remote_exec::{ActionRequest, ActionResult, RemoteExecutor};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingStrategy {
    LeastLoaded,
    Random,
    RoundRobin,
    DataLocality,
}

pub struct Scheduler {
    strategy: SchedulingStrategy,
    worker_endpoints: Arc<Mutex<HashMap<String, String>>>, // worker_id -> endpoint
    next_worker_idx: Mutex<usize>,
}

impl Scheduler {
    pub fn new(strategy: SchedulingStrategy) -> Self {
        Self {
            strategy,
            worker_endpoints: Arc::new(Mutex::new(HashMap::new())),
            next_worker_idx: Mutex::new(0),
        }
    }

    pub async fn register_worker(&self, worker_id: String, endpoint: String) {
        let mut endpoints = self.worker_endpoints.lock().await;
        endpoints.insert(worker_id.clone(), endpoint);
        println!("📝 Scheduler registered worker: {}", worker_id);
    }

    pub async fn get_available_workers(&self) -> Vec<(String, String)> {
        let endpoints = self.worker_endpoints.lock().await;
        endpoints
            .iter()
            .map(|(id, endpoint)| (id.clone(), endpoint.clone()))
            .collect()
    }

    async fn select_worker(&self, action: &ActionRequest) -> Result<String> {
        let workers = self.get_available_workers().await;
        if workers.is_empty() {
            return Err(anyhow!("No available workers registered with scheduler"));
        }

        match self.strategy {
            SchedulingStrategy::Random => {
                use rand::Rng;
                let idx = rand::thread_rng().gen_range(0..workers.len());
                Ok(workers[idx].1.clone())
            }
            SchedulingStrategy::RoundRobin => {
                let mut next = self.next_worker_idx.lock().await;
                let worker = &workers[*next % workers.len()];
                *next += 1;
                Ok(worker.1.clone())
            }
            SchedulingStrategy::LeastLoaded => {
                // For MVP, fallback to RoundRobin
                let mut next = self.next_worker_idx.lock().await;
                let worker = &workers[*next % workers.len()];
                *next += 1;
                Ok(worker.1.clone())
            }
            SchedulingStrategy::DataLocality => {
                // Consistent hashing based on input root digest
                let hash_val = blake3::hash(action.input_root_digest.hash.as_bytes());
                let bytes = hash_val.as_bytes();
                let idx = (bytes[0] as usize + ((bytes[1] as usize) << 8)) % workers.len();
                Ok(workers[idx].1.clone())
            }
        }
    }
}

#[async_trait]
impl RemoteExecutor for Scheduler {
    async fn execute(&self, action: ActionRequest) -> Result<ActionResult> {
        let worker_endpoint = self.select_worker(&action).await?;
        println!("🎯 Dispatching action to worker: {}", worker_endpoint);

        // Create a client executor for the selected worker
        let client = crate::remote_exec::client::RemoteExecClient::new(&worker_endpoint);
        client.execute(action).await
    }
}
