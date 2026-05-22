//! MemoBuild Operator Controller
//!
//! This module implements the main reconciliation loop for the MemoBuild operator.
//! It watches for MemoBuildCluster resources and ensures the actual state matches
//! the desired state defined in the CRD.

use crate::operator::crd::{MemoBuildCluster, MemoBuildClusterSpec, MemoBuildClusterStatus, ClusterCondition};
use anyhow::Result;
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{Service, ServiceSpec, ConfigMap, Secret, PodTemplateSpec, PodSpec, Container, EnvVar, EnvFromSource, SecretVolumeSource, Volume, VolumeMount};
use kube::api::{ObjectMeta, ListParams, Patch, PatchParams};
use kube::runtime::controller::{Action, Controller};
use kube::runtime::events::{Recorder, Event};
use kube::runtime::finalizer::{finalizer, Finalizer};
use kube::{Api, Client, Resource, ResourceExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{error, info, warn};

const FINALIZER: &str = "memobuildcluster.finalizer.build.nrelab.io";

pub struct OperatorContext {
    pub client: Client,
    pub statefulsets: Api<StatefulSet>,
    pub services: Api<Service>,
    pub configmaps: Api<ConfigMap>,
    pub secrets: Api<Secret>,
    pub clusters: Api<MemoBuildCluster>,
}

impl OperatorContext {
    pub fn new(client: Client, namespace: &str) -> Self {
        Self {
            client: client.clone(),
            statefulsets: Api::namespaced(client.clone(), namespace),
            services: Api::namespaced(client.clone(), namespace),
            configmaps: Api::namespaced(client.clone(), namespace),
            secrets: Api::namespaced(client.clone(), namespace),
            clusters: Api::namespaced(client.clone(), namespace),
        }
    }
}

pub async fn run_operator(namespace: &str) -> Result<()> {
    let client = Client::try_default().await?;
    let ctx = Arc::new(OperatorContext::new(client.clone(), namespace));
    
    let clusters: Api<MemoBuildCluster> = Api::namespaced(client, namespace);
    let controller = Controller::new(clusters, ListParams::default())
        .run(
            |cluster: Arc<MemoBuildCluster>, ctx: Arc<OperatorContext>| {
                let ctx = ctx.clone();
                async move {
                    reconcile(cluster, ctx).await
                }
            },
            |cluster: Arc<MemoBuildCluster>, ctx: Arc<OperatorContext>, error: &anyhow::Error| {
                error!("Reconciliation failed for {}: {}", cluster.name_any(), error);
                Action::requeue(Duration::from_secs(30))
            },
        )
        .await;

    Ok(())
}

async fn reconcile(cluster: Arc<MemoBuildCluster>, ctx: Arc<OperatorContext>) -> Result<Action> {
    let name = cluster.name_any();
    let ns = cluster.namespace().unwrap_or_default();
    
    info!("Reconciling MemoBuildCluster {}", name);

    let finalizer = Finalizer::new(FINALIZER);
    finalizer
        .handle_event(&ctx.clusters, &cluster, |event| async {
            match event {
                kube::runtime::finalizer::Event::Apply(cluster) => {
                    reconcile_cluster(cluster, ctx.clone()).await
                }
                kube::runtime::finalizer::Event::Cleanup(cluster) => {
                    cleanup_cluster(cluster, ctx.clone()).await
                }
            }
        })
        .await?;

    Ok(Action::requeue(Duration::from_secs(60)))
}

async fn reconcile_cluster(cluster: MemoBuildCluster, ctx: Arc<OperatorContext>) -> Result<Action> {
    let name = cluster.name_any();
    let ns = cluster.namespace().unwrap_or_default();
    let spec = &cluster.spec;
    
    let replicas = spec.replicas.unwrap_or(3);
    let image = spec.image.as_ref().unwrap();
    let repo = image.repository.as_deref().unwrap_or("ghcr.io/nrelab/memobuild");
    let tag = image.tag.as_deref().unwrap_or("latest");
    let pull_policy = image.pull_policy.as_deref().unwrap_or("IfNotPresent");
    
    // 1. Ensure ConfigMap exists
    let configmap = build_configmap(&name, spec);
    match ctx.configmaps.get(&name).await {
        Ok(_) => {
            ctx.configmaps
                .patch(&name, &PatchParams::apply("memobuild-operator"), &configmap)
                .await?;
            info!("Updated ConfigMap {}", name);
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            ctx.configmaps.create(&configmap).await?;
            info!("Created ConfigMap {}", name);
        }
        Err(e) => return Err(e.into()),
    }

    // 2. Ensure Secret exists (if TLS is configured)
    if let Some(tls_ref) = &spec.tls_secret_ref {
        if let Some(secret_name) = &tls_ref.name {
            ensure_tls_secret(&ctx, &name, secret_name).await?;
        }
    }

    // 3. Ensure Service exists
    let service = build_service(&name, spec);
    match ctx.services.get(&name).await {
        Ok(_) => {
            ctx.services
                .patch(&name, &PatchParams::apply("memobuild-operator"), &service)
                .await?;
            info!("Updated Service {}", name);
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            ctx.services.create(&service).await?;
            info!("Created Service {}", name);
        }
        Err(e) => return Err(e.into()),
    }

    // 4. Ensure StatefulSet exists
    let statefulset = build_statefulset(&name, repo, tag, pull_policy, spec);
    match ctx.statefulsets.get(&name).await {
        Ok(_) => {
            ctx.statefulsets
                .patch(&name, &PatchParams::apply("memobuild-operator"), &statefulset)
                .await?;
            info!("Updated StatefulSet {}", name);
        }
        Err(kube::Error::Api(e)) if e.code == 404 => {
            ctx.statefulsets.create(&statefulset).await?;
            info!("Created StatefulSet {}", name);
        }
        Err(e) => return Err(e.into()),
    }

    // 5. Update status
    let status = MemoBuildClusterStatus {
        ready_replicas: Some(replicas),
        replicas: Some(replicas),
        conditions: vec![ClusterCondition {
            condition_type: "Ready".to_string(),
            status: "True".to_string(),
            last_transition_time: Some(chrono::Utc::now().to_rfc3339()),
            reason: Some("ReconciliationComplete".to_string()),
            message: Some("All resources created successfully".to_string()),
        }],
    };

    let cluster_copy = cluster.clone();
    let _ = ctx.clusters
        .patch_status(&name, &PatchParams::apply("memobuild-operator"), &cluster_copy)
        .await;

    Ok(Action::requeue(Duration::from_secs(60)))
}

async fn cleanup_cluster(cluster: MemoBuildCluster, ctx: Arc<OperatorContext>) -> Result<Action> {
    let name = cluster.name_any();
    info!("Cleaning up MemoBuildCluster {}", name);

    // Delete StatefulSet
    if let Err(e) = ctx.statefulsets.delete(&name, &Default::default()).await {
        warn!("Failed to delete StatefulSet: {}", e);
    }

    // Delete Service
    if let Err(e) = ctx.services.delete(&name, &Default::default()).await {
        warn!("Failed to delete Service: {}", e);
    }

    // Delete ConfigMap
    if let Err(e) = ctx.configmaps.delete(&name, &Default::default()).await {
        warn!("Failed to delete ConfigMap: {}", e);
    }

    Ok(Action::await_finalization())
}

fn build_configmap(name: &str, spec: &MemoBuildClusterSpec) -> ConfigMap {
    let mut data = HashMap::new();
    
    data.insert("MEMOBUILD_STORAGE_BACKEND".to_string(), 
        spec.storage_backend.clone().unwrap_or_else(|| "s3".to_string()));
    
    if let Some(storage) = &spec.storage_config {
        if let Some(bucket) = &storage.bucket {
            data.insert("MEMOBUILD_STORAGE_BUCKET".to_string(), bucket.clone());
        }
        if let Some(endpoint) = &storage.endpoint {
            data.insert("MEMOBUILD_STORAGE_ENDPOINT".to_string(), endpoint.clone());
        }
        if let Some(region) = &storage.region {
            data.insert("MEMOBUILD_STORAGE_REGION".to_string(), region.clone());
        }
    }

    if let Some(pg) = &spec.postgres_ref {
        if let Some(host) = &pg.host {
            data.insert("MEMOBUILD_DATABASE_URL".to_string(), 
                format!("postgresql://{}@{}:{}/{}", 
                    pg.user.as_deref().unwrap_or("memobuild"),
                    host, 
                    pg.port.unwrap_or(5432),
                    pg.database.as_deref().unwrap_or("memobuild")
                ));
        }
    }

    if let Some(redis) = &spec.redis_ref {
        if let Some(host) = &redis.host {
            data.insert("MEMOBUILD_REDIS_URL".to_string(), 
                format!("redis://{}:{}", host, redis.port.unwrap_or(6379)));
        }
    }

    ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some({
                let mut labels = HashMap::new();
                labels.insert("app".to_string(), name.to_string());
                labels
            }),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

fn build_service(name: &str, _spec: &MemoBuildClusterSpec) -> Service {
    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some({
                let mut labels = HashMap::new();
                labels.insert("app".to_string(), name.to_string());
                labels
            }),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some({
                let mut selector = HashMap::new();
                selector.insert("app".to_string(), name.to_string());
                selector
            }),
            ports: Some(vec![
                k8s_openapi::api::core::v1::ServicePort {
                    port: 8080,
                    target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8080)),
                    name: Some("http".to_string()),
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
                k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(9090),
                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(9090)),
                name: Some("metrics".to_string()),
                protocol: Some("TCP".to_string()),
            ]),
            cluster_ip: Some("None".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_statefulset(name: &str, repo: &str, tag: &str, pull_policy: &str, spec: &MemoBuildClusterSpec) -> StatefulSet {
    let replicas = spec.replicas.unwrap_or(3);
    
    let mut env = vec![
        EnvVar {
            name: "MEMOBUILD_NODE_ID".to_string(),
            value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                field_ref: Some(k8s_openapi::api::core::v1::ObjectFieldSelector {
                    field_path: "metadata.name".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    if let Some(policy) = &spec.scaling_policy {
        env.push(EnvVar {
            name: "MEMOBUILD_SCALING_MIN".to_string(),
            value: Some(policy.min_replicas.map(|v| v.to_string()).unwrap_or_default()),
            ..Default::default()
        });
        env.push(EnvVar {
            name: "MEMOBUILD_SCALING_MAX".to_string(),
            value: Some(policy.max_replicas.map(|v| v.to_string()).unwrap_or_default()),
            ..Default::default()
        });
    }

    let mut volumes = vec![];
    let mut volume_mounts = vec![];
    
    volumes.push(Volume {
        name: "config".to_string(),
        config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
            name: Some(name.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    });
    volume_mounts.push(VolumeMount {
        name: "config".to_string(),
        mount_path: "/etc/memobuild".to_string(),
        ..Default::default()
    });

    StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            labels: Some({
                let mut labels = HashMap::new();
                labels.insert("app".to_string(), name.to_string());
                labels
            }),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(replicas),
            selector: Some({
                let mut selector = HashMap::new();
                selector.insert("app".to_string(), name.to_string());
                selector
            }),
            service_name: name.to_string(),
            pod_management_policy: Some("Parallel".to_string()),
            update_strategy: Some(k8s_openapi::api::apps::v1::StatefulSetUpdateStrategy {
                rolling_update: Some(k8s_openapi::api::apps::v1::RollingUpdateStatefulSetStrategy {
                    max_unavailable: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(1)),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some({
                        let mut labels = HashMap::new();
                        labels.insert("app".to_string(), name.to_string());
                        labels
                    }),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "memobuild".to_string(),
                        image: Some(format!("{}:{}", repo, tag)),
                        image_pull_policy: Some(pull_policy.to_string()),
                        ports: Some(vec![
                            k8s_openapi::api::core::v1::ContainerPort {
                                container_port: 8080,
                                name: Some("http".to_string()),
                                protocol: Some("TCP".to_string()),
                                ..Default::default()
                            },
                            k8s_openapi::api::core::v1::ContainerPort {
                                container_port: 9090,
                                name: Some("metrics".to_string()),
                                protocol: Some("TCP".to_string()),
                                ..Default::default()
                            },
                        ]),
                        env: Some(env),
                        env_from: Some(vec![
                            EnvFromSource {
                                config_map_ref: Some(k8s_openapi::api::core::v1::ConfigMapEnvSource {
                                    name: Some(name.to_string()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                        ]),
                        volume_mounts: Some(volume_mounts),
                        resources: spec.resources.as_ref().map(|r| k8s_openapi::api::core::v1::ResourceRequirements {
                            requests: r.requests.as_ref().map(|m| {
                                m.iter().map(|(k, v)| (k.clone(), k8s_openapi::apimachinery::pkg::api::resource::Quantity(v.clone()))).collect()
                            }),
                            limits: r.limits.as_ref().map(|m| {
                                m.iter().map(|(k, v)| (k.clone(), k8s_openapi::apimachinery::pkg::api::resource::Quantity(v.clone()))).collect()
                            }),
                            ..Default::default()
                        }),
                        readiness_probe: Some(k8s_openapi::api::core::v1::Probe {
                            http_get: Some(k8s_openapi::api::core::v1::HTTPGetAction {
                                path: Some("/health".to_string()),
                                port: k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8080),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(10),
                            period_seconds: Some(5),
                            ..Default::default()
                        }),
                        liveness_probe: Some(k8s_openapi::api::core::v1::Probe {
                            http_get: Some(k8s_openapi::api::core::v1::HTTPGetAction {
                                path: Some("/health".to_string()),
                                port: k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(8080),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(30),
                            period_seconds: Some(10),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    volumes: Some(volumes),
                    node_selector: spec.node_selector.clone(),
                    tolerations: spec.tolerations.clone(),
                    affinity: spec.affinity.as_ref().map(|a| k8s_openapi::api::core::v1::Affinity {
                        node_affinity: a.node_affinity.as_ref().map(|na| k8s_openapi::api::core::v1::NodeAffinity {
                            required_during_scheduling_ignored_during_execution: na.required_during_scheduling_ignored_during_execution.map(|r| k8s_openapi::api::core::v1::NodeSelector {
                                node_selector_terms: r.node_selector_terms.iter().map(|t| k8s_openapi::api::core::v1::NodeSelectorTerm {
                                    match_expressions: t.match_expressions.as_ref().map(|e| e.iter().map(|r| k8s_openapi::api::core::v1::NodeSelectorRequirement {
                                        key: r.key.clone(),
                                        operator: r.operator.clone(),
                                        values: r.values.clone(),
                                        ..Default::default()
                                    }).collect()),
                                    match_fields: t.match_fields.as_ref().map(|e| e.iter().map(|r| k8s_openapi::api::core::v1::NodeSelectorRequirement {
                                        key: r.key.clone(),
                                        operator: r.operator.clone(),
                                        values: r.values.clone(),
                                        ..Default::default()
                                    }).collect()),
                                    ..Default::default()
                                }).collect()),
                                ..Default::default()
                            }),
                            preferred_during_scheduling_ignored_during_execution: na.preferred_during_scheduling_ignored_during_execution.as_ref().map(|p| p.iter().map(|t| k8s_openapi::api::core::v1::PreferredSchedulingTerm {
                                weight: t.weight,
                                preference: k8s_openapi::api::core::v1::NodeSelectorTerm {
                                    match_expressions: t.preference.match_expressions.as_ref().map(|e| e.iter().map(|r| k8s_openapi::api::core::v1::NodeSelectorRequirement {
                                        key: r.key.clone(),
                                        operator: r.operator.clone(),
                                        values: r.values.clone(),
                                        ..Default::default()
                                    }).collect()),
                                    match_fields: t.preference.match_fields.as_ref().map(|e| e.iter().map(|r| k8s_openapi::api::core::v1::NodeSelectorRequirement {
                                        key: r.key.clone(),
                                        operator: r.operator.clone(),
                                        values: r.values.clone(),
                                        ..Default::default()
                                    }).collect()),
                                    ..Default::default()
                                },
                            }).collect()),
                            ..Default::default()
                        }),
                        pod_affinity: None,
                        pod_anti_affinity: None,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn ensure_tls_secret(ctx: &OperatorContext, cluster_name: &str, secret_name: &str) -> Result<()> {
    // TLS secret would be managed by cert-manager in production
    // This is a placeholder for the operator
    Ok(())
}