//! Auto-scaling for Kubernetes Integration
//!
//! This module provides Horizontal Pod Autoscaler (HPA) integration,
//! queue-based scaling triggers, and resource prediction algorithms.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metrics collected for scaling decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingMetrics {
    pub timestamp: DateTime<Utc>,
    pub active_builds: u32,
    pub queued_builds: u32,
    pub worker_utilization: f64, // 0.0 to 1.0
    pub cache_hit_rate: f64,     // 0.0 to 1.0
    pub avg_build_time_ms: u64,
    pub memory_usage_mb: u64,
    pub cpu_usage_percent: f64,
}

/// Scaling policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingPolicy {
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub target_utilization_percent: f64,
    pub scale_up_threshold: f64,
    pub scale_down_threshold: f64,
    pub stabilization_window_secs: u64,
    pub cooldown_period_secs: u64,
}

/// Auto-scaling engine for distributed build system
pub struct AutoScaler {
    metrics_history: Arc<RwLock<VecDeque<ScalingMetrics>>>,
    current_replicas: Arc<RwLock<u32>>,
    policy: ScalingPolicy,
    last_scale_time: Arc<RwLock<DateTime<Utc>>>,
    k8s_client: Option<kube::Client>,
}

impl AutoScaler {
    pub async fn new(policy: ScalingPolicy) -> Result<Self> {
        Ok(Self {
            metrics_history: Arc::new(RwLock::new(VecDeque::with_capacity(100))),
            current_replicas: Arc::new(RwLock::new(policy.min_replicas)),
            policy,
            last_scale_time: Arc::new(RwLock::new(Utc::now())),
            k8s_client: None, // Will be initialized if running in cluster
        })
    }

    /// Initialize Kubernetes client for HPA integration
    pub async fn with_kubernetes(mut self) -> Result<Self> {
        if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
            self.k8s_client = Some(kube::Client::try_default().await?);
        }
        Ok(self)
    }

    /// Record new metrics for scaling decisions
    pub async fn record_metrics(&self, metrics: ScalingMetrics) -> Result<()> {
        let mut history = self.metrics_history.write().await;

        // Keep only recent metrics (last 10 minutes)
        let cutoff = Utc::now() - chrono::Duration::seconds(600);
        while let Some(old) = history.front() {
            if old.timestamp < cutoff {
                history.pop_front();
            } else {
                break;
            }
        }

        history.push_back(metrics);

        // Check if scaling is needed
        self.evaluate_scaling().await?;

        Ok(())
    }

    /// Evaluate if scaling is needed based on current metrics
    async fn evaluate_scaling(&self) -> Result<()> {
        let history = self.metrics_history.read().await;
        if history.len() < 5 {
            return Ok(()); // Need minimum data points
        }

        let current_replicas = *self.current_replicas.read().await;
        let last_scale = *self.last_scale_time.read().await;

        // Check cooldown period
        if Utc::now() - last_scale
            < chrono::Duration::seconds(self.policy.cooldown_period_secs as i64)
        {
            return Ok(());
        }

        // Calculate average metrics over stabilization window
        let recent_metrics: Vec<_> = history
            .iter()
            .rev()
            .take(self.policy.stabilization_window_secs as usize)
            .collect();

        let avg_utilization = recent_metrics
            .iter()
            .map(|m| m.worker_utilization)
            .sum::<f64>()
            / recent_metrics.len() as f64;
        let avg_queued = recent_metrics
            .iter()
            .map(|m| m.queued_builds as f64)
            .sum::<f64>()
            / recent_metrics.len() as f64;

        // Scale up conditions
        let should_scale_up = avg_utilization > self.policy.scale_up_threshold || avg_queued > 5.0; // More than 5 queued builds

        // Scale down conditions
        let should_scale_down = avg_utilization < self.policy.scale_down_threshold
            && avg_queued < 1.0
            && current_replicas > self.policy.min_replicas;

        let target_replicas = if should_scale_up && current_replicas < self.policy.max_replicas {
            current_replicas + 1
        } else if should_scale_down {
            current_replicas - 1
        } else {
            current_replicas
        };

        if target_replicas != current_replicas {
            self.scale_to(target_replicas).await?;
        }

        Ok(())
    }

    /// Scale to target number of replicas
    async fn scale_to(&self, target_replicas: u32) -> Result<()> {
        println!(
            "🔄 Scaling from {} to {} replicas",
            *self.current_replicas.read().await,
            target_replicas
        );

        *self.current_replicas.write().await = target_replicas;
        *self.last_scale_time.write().await = Utc::now();

        // Update Kubernetes HPA if available
        if let Some(client) = &self.k8s_client {
            self.update_kubernetes_hpa(client, target_replicas).await?;
        }

        Ok(())
    }

    /// Update Kubernetes HorizontalPodAutoscaler
    async fn update_kubernetes_hpa(&self, client: &kube::Client, replicas: u32) -> Result<()> {
        use k8s_openapi::api::autoscaling::v1::HorizontalPodAutoscaler;
        use kube::api::{Api, Patch, PatchParams};

        let hpa_api: Api<HorizontalPodAutoscaler> = Api::default_namespaced(client.clone());

        // Patch the HPA with new min replicas
        let patch = serde_json::json!({
            "spec": {
                "minReplicas": replicas
            }
        });

        hpa_api
            .patch(
                "memobuild-scheduler",
                &PatchParams::default(),
                &Patch::Merge(&patch),
            )
            .await?;

        println!("✅ Updated Kubernetes HPA to {} min replicas", replicas);
        Ok(())
    }

    /// Get current scaling status
    pub async fn get_scaling_status(&self) -> Result<ScalingStatus> {
        let current_replicas = *self.current_replicas.read().await;
        let history = self.metrics_history.read().await;

        let avg_utilization = if history.is_empty() {
            0.0
        } else {
            history.iter().map(|m| m.worker_utilization).sum::<f64>() / history.len() as f64
        };

        Ok(ScalingStatus {
            current_replicas,
            target_replicas: current_replicas, // For now, current == target
            average_utilization: avg_utilization,
            last_scale_time: *self.last_scale_time.read().await,
            policy: self.policy.clone(),
        })
    }

    /// Predict resource needs based on historical data
    pub async fn predict_resource_needs(
        &self,
        time_window_secs: u64,
    ) -> Result<ResourcePrediction> {
        let history = self.metrics_history.read().await;

        if history.is_empty() {
            return Ok(ResourcePrediction {
                predicted_replicas: self.policy.min_replicas,
                confidence: 0.0,
                reasoning: "No historical data available".to_string(),
            });
        }

        // Simple linear regression on utilization
        let recent: Vec<_> = history
            .iter()
            .rev()
            .take(time_window_secs as usize)
            .enumerate()
            .map(|(i, m)| (i as f64, m.worker_utilization))
            .collect();

        if recent.len() < 2 {
            return Ok(ResourcePrediction {
                predicted_replicas: self.policy.min_replicas,
                confidence: 0.5,
                reasoning: "Insufficient data for prediction".to_string(),
            });
        }

        // Calculate trend (simplified)
        let n = recent.len() as f64;
        let sum_x: f64 = recent.iter().map(|(x, _)| x).sum();
        let sum_y: f64 = recent.iter().map(|(_, y)| y).sum();
        let sum_xy: f64 = recent.iter().map(|(x, y)| x * y).sum();
        let sum_xx: f64 = recent.iter().map(|(x, _)| x * x).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
        let intercept = (sum_y - slope * sum_x) / n;

        // Predict next value
        let next_x = recent.last().unwrap().0 + 1.0;
        let predicted_utilization = slope * next_x + intercept;

        // Convert to replica count
        let predicted_replicas = if predicted_utilization
            > self.policy.target_utilization_percent / 100.0
        {
            ((predicted_utilization * 100.0 / self.policy.target_utilization_percent).ceil() as u32)
                .max(self.policy.min_replicas)
                .min(self.policy.max_replicas)
        } else {
            self.policy.min_replicas
        };

        let confidence = if recent.len() > 10 { 0.8 } else { 0.6 };

        Ok(ResourcePrediction {
            predicted_replicas,
            confidence,
            reasoning: format!(
                "Predicted utilization: {:.1}%, slope: {:.3}",
                predicted_utilization * 100.0,
                slope
            ),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingStatus {
    pub current_replicas: u32,
    pub target_replicas: u32,
    pub average_utilization: f64,
    pub last_scale_time: DateTime<Utc>,
    pub policy: ScalingPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePrediction {
    pub predicted_replicas: u32,
    pub confidence: f64, // 0.0 to 1.0
    pub reasoning: String,
}

/// Queue-based scaling trigger for reactive scaling
pub struct QueueBasedScaler {
    build_queue: Arc<RwLock<VecDeque<BuildRequest>>>,
    scaler: Arc<AutoScaler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRequest {
    pub id: String,
    pub priority: u8, // 0 = lowest, 255 = highest
    pub estimated_duration_ms: u64,
    pub submitted_at: DateTime<Utc>,
}

impl QueueBasedScaler {
    pub fn new(scaler: Arc<AutoScaler>) -> Self {
        Self {
            build_queue: Arc::new(RwLock::new(VecDeque::new())),
            scaler,
        }
    }

    /// Add build request to queue
    pub async fn enqueue_build(&self, request: BuildRequest) -> Result<()> {
        let mut queue = self.build_queue.write().await;
        queue.push_back(request);

        // Trigger scaling evaluation
        self.evaluate_queue_scaling().await?;

        Ok(())
    }

    /// Remove completed build from queue
    pub async fn dequeue_build(&self, build_id: &str) -> Result<Option<BuildRequest>> {
        let mut queue = self.build_queue.write().await;

        if let Some(pos) = queue.iter().position(|r| r.id == build_id) {
            let request = queue.remove(pos).unwrap();
            return Ok(Some(request));
        }

        Ok(None)
    }

    /// Evaluate scaling based on queue depth
    async fn evaluate_queue_scaling(&self) -> Result<()> {
        let queue = self.build_queue.read().await;
        let queue_depth = queue.len() as f64;

        // Calculate queue metrics
        let _avg_wait_time = if !queue.is_empty() {
            let now = Utc::now();
            let total_wait: i64 = queue
                .iter()
                .map(|r| (now - r.submitted_at).num_milliseconds())
                .sum();
            total_wait as f64 / queue.len() as f64
        } else {
            0.0
        };

        // Create scaling metrics
        let metrics = ScalingMetrics {
            timestamp: Utc::now(),
            active_builds: 0, // Would come from worker status
            queued_builds: queue_depth as u32,
            worker_utilization: 0.0, // Would come from worker metrics
            cache_hit_rate: 0.0,     // Would come from cache metrics
            avg_build_time_ms: 0,    // Would come from completed builds
            memory_usage_mb: 0,      // Would come from system metrics
            cpu_usage_percent: 0.0,  // Would come from system metrics
        };

        // Record metrics for scaling decision
        self.scaler.record_metrics(metrics).await?;

        Ok(())
    }

    /// Get queue statistics
    pub async fn get_queue_stats(&self) -> Result<QueueStats> {
        let queue = self.build_queue.read().await;

        let total_queued = queue.len();
        let avg_wait_time_ms = if !queue.is_empty() {
            let now = Utc::now();
            let total_wait: i64 = queue
                .iter()
                .map(|r| (now - r.submitted_at).num_milliseconds())
                .sum();
            (total_wait / queue.len() as i64) as u64
        } else {
            0
        };

        let priority_distribution: std::collections::HashMap<u8, usize> =
            queue
                .iter()
                .fold(std::collections::HashMap::new(), |mut acc, req| {
                    *acc.entry(req.priority).or_insert(0) += 1;
                    acc
                });

        Ok(QueueStats {
            total_queued,
            avg_wait_time_ms,
            priority_distribution,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub total_queued: usize,
    pub avg_wait_time_ms: u64,
    pub priority_distribution: std::collections::HashMap<u8, usize>,
}