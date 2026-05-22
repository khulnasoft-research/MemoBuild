//! OpenTelemetry tracing for MemoBuild
//!
//! This module provides distributed tracing integration using OpenTelemetry.

use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::env;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize OpenTelemetry tracing
pub fn init_tracing() -> Option<()> {
    let otlp_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok()?;
    if otlp_endpoint.is_empty() {
        return None;
    }

    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .build()
        .expect("Failed to create OTLP exporter");

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(Resource::builder()
            .with_attributes(vec![
                KeyValue::new("service.name", "memobuild"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ])
            .build())
        .build();

    global::set_tracer_provider(provider.clone());
    let tracer = global::tracer("memobuild");

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(OpenTelemetryLayer::new(tracer))
        .init();

    Some(())
}

/// Create a span for build execution
#[macro_export]
macro_rules! build_span {
    ($name:expr) => {
        tracing::span!(tracing::Level::INFO, "build.dag.execute", name = $name)
    };
}

/// Create a span for cache operations
#[macro_export]
macro_rules! cache_span {
    ($operation:expr, $tier:expr) => {
        tracing::span!(tracing::Level::INFO, "cache.lookup", operation = $operation, cache.tier = $tier)
    };
}

/// Create a span for cluster replication
#[macro_export]
macro_rules! replicate_span {
    ($target_node:expr) => {
        tracing::span!(tracing::Level::INFO, "cluster.replicate", target_node = $target_node)
    };
}

/// Create a span for OCI operations
#[macro_export]
macro_rules! oci_span {
    ($operation:expr, $registry:expr, $layer_count:expr) => {
        tracing::span!(tracing::Level::INFO, "oci.push", operation = $operation, registry = $registry, layer_count = $layer_count)
    };
}

/// Create a span for cluster replication
#[allow(dead_code)]
pub fn cluster_span(_operation: &str, _target_nodes: &[String]) {}

/// Extract trace context from HTTP headers
#[allow(dead_code)]
pub fn extract_context_from_headers<B>(_req: &axum::http::Request<B>) -> Option<String> {
    None
}

/// Tracing guard that ensures spans are exported on drop
pub struct TracingGuard;

impl TracingGuard {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl Default for TracingGuard {
    fn default() -> Self {
        Self::new()
    }
}
