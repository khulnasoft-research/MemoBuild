//! MemoBuild Kubernetes Operator
//!
//! This module implements a Kubernetes operator for managing MemoBuild clusters.
//! It uses the kube-rs framework to watch for MemoBuildCluster resources and
//! reconcile them with the desired state (StatefulSet, Services, ConfigMaps, etc.).

pub mod controller;
pub mod crd;

use serde::{Deserialize, Serialize};

/// MemoBuildClusterSpec defines the desired state of MemoBuildCluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoBuildClusterSpec {
    pub replicas: Option<i32>,
    pub replication_factor: Option<i32>,
    pub image: Option<ImageSpec>,
    pub storage_backend: Option<String>,
    pub storage_config: Option<StorageConfig>,
    pub tls_secret_ref: Option<TlsSecretRef>,
    pub postgres_ref: Option<PostgresRef>,
    pub redis_ref: Option<RedisRef>,
    pub scaling_policy: Option<ScalingPolicy>,
    pub resources: Option<ResourceRequirements>,
    pub node_selector: Option<std::collections::HashMap<String, String>>,
    pub tolerations: Option<Vec<Toleration>>,
    pub affinity: Option<Affinity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSpec {
    pub repository: Option<String>,
    pub tag: Option<String>,
    pub pull_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub bucket: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSecretRef {
    pub name: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub ca_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresRef {
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub secret_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisRef {
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub secret_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingPolicy {
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub target_cpu_percent: Option<i32>,
    pub target_memory_percent: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub requests: Option<std::collections::HashMap<String, String>>,
    pub limits: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toleration {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affinity {
    pub node_affinity: Option<NodeAffinity>,
    pub pod_affinity: Option<PodAffinity>,
    pub pod_anti_affinity: Option<PodAntiAffinity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAffinity {
    pub required_during_scheduling_ignored_during_execution: Option<NodeSelector>,
    pub preferred_during_scheduling_ignored_during_execution: Option<Vec<PreferredSchedulingTerm>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSelector {
    pub node_selector_terms: Vec<NodeSelectorTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSelectorTerm {
    pub match_expressions: Option<Vec<NodeSelectorRequirement>>,
    pub match_fields: Option<Vec<NodeSelectorRequirement>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSelectorRequirement {
    pub key: String,
    pub operator: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferredSchedulingTerm {
    pub weight: i32,
    pub preference: NodeSelectorTerm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodAffinity {
    pub required_during_scheduling_ignored_during_execution: Vec<PodAffinityTerm>,
    pub preferred_during_scheduling_ignored_during_execution: Vec<WeightedPodAffinityTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodAffinityTerm {
    pub label_selector: Option<LabelSelector>,
    pub namespaces: Option<Vec<String>>,
    pub topology_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSelector {
    pub match_labels: Option<std::collections::HashMap<String, String>>,
    pub match_expressions: Option<Vec<LabelSelectorRequirement>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSelectorRequirement {
    pub key: String,
    pub operator: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedPodAffinityTerm {
    pub weight: i32,
    pub pod_affinity_term: PodAffinityTerm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodAntiAffinity {
    pub required_during_scheduling_ignored_during_execution: Vec<PodAffinityTerm>,
    pub preferred_during_scheduling_ignored_during_execution: Vec<WeightedPodAffinityTerm>,
}

/// MemoBuildClusterStatus defines the observed state of MemoBuildCluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoBuildClusterStatus {
    pub ready_replicas: Option<i32>,
    pub replicas: Option<i32>,
    pub conditions: Vec<ClusterCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterCondition {
    pub condition_type: String,
    pub status: String,
    pub last_transition_time: Option<String>,
    pub reason: Option<String>,
    pub message: Option<String>,
}

impl Default for MemoBuildClusterStatus {
    fn default() -> Self {
        Self {
            ready_replicas: Some(0),
            replicas: Some(0),
            conditions: vec![],
        }
    }
}

/// K8s API object traits for the operator
pub mod k8s {
    use super::*;
    use kube::CustomResource;
    use kube::core::ObjectMeta;

    #[derive(CustomResource, Debug, Clone, Serialize, Deserialize)]
    #[kube(group = "build.nrelab.io", version = "v1alpha1", kind = "MemoBuildCluster", plural = "memobuildclusters")]
    #[kube(namespaced)]
    #[kube(status = "MemoBuildClusterStatus")]
    pub struct MemoBuildCluster {
        pub spec: MemoBuildClusterSpec,
        pub status: Option<MemoBuildClusterStatus>,
    }

    impl MemoBuildCluster {
        pub fn new(name: &str, namespace: &str) -> Self {
            Self {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    namespace: Some(namespace.to_string()),
                    ..Default::default()
                },
                spec: MemoBuildClusterSpec::default(),
                status: None,
                // Need to add api_version and kind
            }
        }
    }

    impl Default for MemoBuildClusterSpec {
        fn default() -> Self {
            Self {
                replicas: Some(3),
                replication_factor: Some(2),
                image: Some(ImageSpec {
                    repository: Some("ghcr.io/nrelab/memobuild".to_string()),
                    tag: Some("latest".to_string()),
                    pull_policy: Some("IfNotPresent".to_string()),
                }),
                storage_backend: Some("s3".to_string()),
                storage_config: Some(StorageConfig {
                    bucket: Some("memobuild-cache".to_string()),
                    endpoint: None,
                    region: Some("us-east-1".to_string()),
                    access_key: None,
                    secret_key: None,
                }),
                tls_secret_ref: None,
                postgres_ref: Some(PostgresRef {
                    enabled: Some(true),
                    host: Some("postgres".to_string()),
                    port: Some(5432),
                    database: Some("memobuild".to_string()),
                    user: Some("memobuild".to_string()),
                    secret_ref: Some("postgres-credentials".to_string()),
                }),
                redis_ref: Some(RedisRef {
                    enabled: Some(true),
                    host: Some("redis".to_string()),
                    port: Some(6379),
                    secret_ref: None,
                }),
                scaling_policy: Some(ScalingPolicy {
                    min_replicas: Some(1),
                    max_replicas: Some(10),
                    target_cpu_percent: Some(70),
                    target_memory_percent: Some(80),
                }),
                resources: Some(ResourceRequirements {
                    requests: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("cpu".to_string(), "500m".to_string());
                        m.insert("memory".to_string(), "1Gi".to_string());
                        m
                    }),
                    limits: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("cpu".to_string(), "2".to_string());
                        m.insert("memory".to_string(), "4Gi".to_string());
                        m
                    }),
                }),
                node_selector: None,
                tolerations: None,
                affinity: None,
            }
        }
    }
}