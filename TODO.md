# MemoBuild: Real-World Containerized Production-Grade Distributed Roadmap
**Baseline:** v0.4.0 (March 2026) → **Target:** v1.0.0 GA (Q4 2026)
**Scope:** Concrete engineering plan based on deep source analysis — not aspirational claims.
## Deep Analysis: Actual State vs. Claimed State
### What Is Real
* **Core DAG engine** (`src/core.rs`, `src/graph.rs`): solid BLAKE3-DAG incremental builds.
* **OCI exporter** (`src/oci/`): functional push/pull via Distribution Spec.
* **HTTP remote cache** (`src/remote_cache.rs`, `src/server/`): axum-based, works for single-region teams.
* **Consistent hashing ring** (`src/cache_cluster.rs`, 383 lines): multi-master replication protocol via HTTP/JSON — no TLS, no auth.
* **PostgreSQL store** (`src/scalable_db.rs`, 425 lines): deadpool connection pool, read replicas — functional but no migrations tooling.
* **Auto-scaler** (`src/auto_scaling.rs`, 415 lines): linear regression prediction + K8s HPA patch — no admission webhook, no PDB.
### Critical Gaps — Still Open
* ~~No multi-arch OCI image build (arm64/amd64) in CI~~ → `.github/workflows/docker.yml` exists with Buildx + QEMU, Cosign signing, SBOM generation.
* GCS storage backend is a local-disk stub — not functional (`src/storage/gcs.rs`).
* `ArtifactStorage` trait missing `stream_get` method prescribed by design.
* No `deploy/grafana/` dashboards or `deploy/prometheus/` alerting rules checked in.
* Missing K8s manifests: `statefulset.yaml`, `priorityclass.yaml`, `podsecuritypolicy.yaml`, standalone `networkpolicy.yaml`.
* Helm chart templates incomplete — missing RBAC, NetworkPolicy, CRD install hook, and subchart deps.
* No Linux landlock/namespace sandboxing (`src/sandbox/linux.rs`) or macOS sandbox (`src/sandbox/macos.rs`).
* Multi-tenancy (`src/tenancy.rs`, `src/admin/`, `src/cdn.rs`) not started.
### Critical Gaps — Now Resolved
* ~~No production Dockerfile~~ → `Dockerfile.server` exists (multi-stage, distroless).
* ~~No Docker Compose~~ → `docker-compose.prod.yml` exists with full stack.
* ~~No Helm chart~~ → `charts/memobuild/` exists (though incomplete).
* ~~DistributedCache no OCI layer replication~~ → `put_layer` replicates to primary + replica nodes.
* ~~No object storage backend~~ → S3 backend exists; GCS is stubbed.
* ~~No Prometheus `/metrics`~~ → endpoint exists in server.
* ~~No mTLS~~ → `src/tls.rs` with rustls, cert-manager manifest.
* ~~No API auth~~ → `src/auth.rs` with Argon2 tokens, rate limiting.
* ~~No SLSA/Cosign~~ → `src/slsa.rs`, `src/verify.rs` implemented.
* ~~No Operator/CRDs~~ → `src/operator/` module + CRD YAML exist.
* ~~No NetworkPolicy/PDB~~ → combined manifest exists; missing PriorityClass.
* ~~No automated GC~~ → `src/gc.rs` with scheduled task + status endpoint.

***
## Phase 0 — Production Containerization (v0.4.1) — 2 weeks [DONE]
**Goal:** Make MemoBuild itself a production-grade OCI artifact before any distributed work proceeds.
### 0.1 Multi-Stage Server Dockerfile
Create `Dockerfile.server` with three stages:
* **Stage 1 `builder`:** `rust:1.82-slim-bookworm` — runs `cargo build --release --locked`.
* **Stage 2 `ca-certs`:** `debian:bookworm-slim` — extracts `/etc/ssl/certs` only.
* **Stage 3 `runtime`:** `gcr.io/distroless/cc-debian12:nonroot` — copies binary + CA certs. Final image ≈ 12 MB.
Build args: `TARGETARCH` for cross-compilation (`cross` crate).
Labels: OCI `org.opencontainers.image.*` annotations, SBOM pointer label.
### 0.2 Docker Compose Full-Stack (`docker-compose.prod.yml`)
Services required for a realistic local distributed environment:
* `memobuild-node1/2/3`: cluster nodes on ports 9090/9091/9092, peer-linked.
* `postgres`: `postgres:16-alpine`, init SQL from `scripts/db/init.sql`, health-checked.
* `redis`: `redis:7-alpine` with `maxmemory-policy allkeys-lru`, for L1 distributed cache.
* `prometheus`: scrapes `/metrics` from all cluster nodes.
* `grafana`: pre-provisioned dashboards via `grafana/provisioning/`.
* `jaeger`: all-in-one for distributed tracing (`OTEL_EXPORTER_OTLP_ENDPOINT`).
* `minio`: S3-compatible blob backend for artifact storage in dev.
Environment variables via `.env.example` with documented entries.
### 0.3 Multi-Arch CI Image Build [DONE]
`.github/workflows/docker.yml` exists with:
* `docker/setup-buildx-action` with QEMU for `linux/amd64,linux/arm64`.
* Build + push to `ghcr.io/nrelab/memobuild:{version,latest,sha}` on tag.
* Cosign keyless signing (`sigstore/cosign-installer`) for each arch-specific digest.
* SBOM generation via `anchore/sbom-action` → attach as OCI referrer.
### 0.4 Fix DistributedCache Layer Replication [VERIFIED FIXED]
`DistributedCache::put_layer` in `src/cache_cluster.rs` already replicates to primary and replica nodes (not a local-only pass-through as previously documented). Verified as fixed.
***
## Phase 1 — Secure Transport Layer (v0.5.0) — 4 weeks [DONE]
**Goal:** All inter-node and client-server communication is authenticated and encrypted.
### 1.1 mTLS for Cluster Nodes
Add `rustls` + `rcgen` to `Cargo.toml`. Boot-time certificate generation or cert-manager-injected volume mount.
* `src/tls.rs`: `TlsConfig` struct — loads `cert.pem`/`key.pem`/`ca.pem` from `MEMOBUILD_TLS_*` env vars.
* Axum server: wrap with `axum_server::tls_rustls::RustlsConfig`.
* reqwest clients (cluster replication, remote cache client): `ClientBuilder::use_rustls_tls().add_root_certificate(ca)`.
* cert-manager `Certificate` CRD manifest in `deploy/k8s/certs/`.
Env vars: `MEMOBUILD_TLS_CERT`, `MEMOBUILD_TLS_KEY`, `MEMOBUILD_TLS_CA`.
### 1.2 API Authentication
Add `src/auth.rs`: Axum middleware layer.
* Bearer token validation (`Authorization: Bearer <token>`) — tokens stored as Argon2-hashed values in PostgreSQL.
* Token issuance endpoint `POST /auth/token` (admin only).
* Rate limiting: `tower_governor` crate, 1000 req/min per token, 100 req/min unauthenticated.
* Audit log: every authenticated operation logged as structured JSON with `tracing::info!`.
Env var: `MEMOBUILD_ADMIN_TOKEN` for bootstrap token.
### 1.3 Secrets Management Integration
* `src/secrets.rs`: trait `SecretProvider` with implementations for:
    * `EnvSecretProvider` (default, dev only)
    * `VaultSecretProvider`: HashiCorp Vault KV v2 via `vaultrs` crate.
    * `AwsKmsProvider`: AWS KMS via `aws-sdk-kms` for registry credential encryption at rest.
* Replace all `env::var("MEMOBUILD_TOKEN")` call sites with `SecretProvider::get("registry_token")`.
### 1.4 Container Security Hardening
In `Dockerfile.server` and all K8s manifests:
* `securityContext.runAsNonRoot: true`, `runAsUser: 65532` (distroless nonroot).
* `readOnlyRootFilesystem: true`.
* `allowPrivilegeEscalation: false`.
* `capabilities.drop: ["ALL"]`.
* Seccomp profile: `RuntimeDefault`.
***
## Phase 2 — Object Storage Backend (v0.5.1) — 3 weeks [PARTIALLY DONE]
**Goal:** Decouple blob storage from local disk. Required for stateless horizontal scaling of cache nodes.
### 2.1 `ArtifactStorage` S3/GCS Backend
Add `src/storage/` module:
* `src/storage/mod.rs`: `ArtifactStorage` trait exists but **missing** `stream_get(&str) -> impl Stream<Item=Bytes>` method.
* `src/storage/s3.rs`: `S3Storage` using `aws-sdk-s3` — multipart upload, presigned URLs.
* `src/storage/gcs.rs`: `GcsStorage` — **STUB only** (all methods write to `/tmp/memobuild-gcs` instead of actual GCS). Needs real `google-cloud-storage` integration.
* `src/storage/local.rs`: existing filesystem backend.
### 2.2 Redis L1 Distributed Cache [DONE]
`src/cache_redis.rs`: `RedisCache` implementing `RemoteCache` via `fred` async Redis client.
* Hot path: cache node checks Redis before hitting object storage. Cache TTL configurable.
* Invalidation: `PUBLISH memobuild:evict:<hash>` on GC.
Config: `MEMOBUILD_REDIS_URL=redis://localhost:6379`.
### 2.3 Automated Garbage Collection [DONE]
`src/gc.rs`: `GarbageCollector` with configurable retention policy (age-based + LRU size-based), scheduled task, GC status endpoint.
***
## Phase 3 — Full Observability Stack (v0.6.0) — 3 weeks [DONE]
**Goal:** Every production metric, trace, and alert is defined in code alongside the source.
### 3.1 Prometheus Metrics Endpoint [DONE]
`src/metrics.rs`: global `MetricsRegistry` with labeled counters/histograms (cache hits/misses, build duration, cluster nodes, replication lag, artifact size, GC deleted). Axum route `GET /metrics` exists.
### 3.2 OpenTelemetry Distributed Tracing [DONE]
`src/tracing.rs`: `init_tracing()` using `opentelemetry-otlp` with OTLP exporter. Span macros: `build_span!`, `cache_span!`, `replicate_span!`, `oci_span!`. `traceparent` header propagation.
### 3.3 Grafana Dashboards as Code [DONE]
* `monitoring/grafana/` directory exists with dashboards (`memobuild-cluster.json`), datasource provisioning, and dashboard provider config.
* `monitoring/prometheus/alert_rules.yml` exists with 6 alert rules (CacheNodeDown, ReplicationLagHigh, BuildQueueSaturated, DiskUsageHigh, ErrorRateHigh, HighMemoryUsage).
* Docker Compose references these configs via `./monitoring/` volume mounts.
***
## Phase 4 — Kubernetes-Native Operator (v0.7.0) — 5 weeks [PARTIALLY DONE]
**Goal:** MemoBuild cluster lifecycle managed by a K8s operator, eliminating manual YAML.
### 4.1 Custom Resource Definitions [DONE]
`deploy/k8s/crds/memobuildcluster.yaml`: CRD for `memobuildclusters.build.nrelab.io`. Spec fields: `replicas`, `storageBackend`, `tlsSecretRef`, `postgresRef`, `redisRef`, `scalingPolicy`.
### 4.2 Operator Implementation [DONE]
`src/operator/` module (`mod.rs`, `crd.rs`, `controller.rs`): reconcile loop, StatefulSet management, HPA patching, K8s Events, leader election.
### 4.3 Production K8s Manifests [PARTIALLY DONE]
`deploy/k8s/` directory structure:
* `manifests/pdb-hpa-network.yaml` — combined PDB + HPA + NetworkPolicy (single file instead of separate manifests).
* **Missing:** standalone `statefulset.yaml`, `podsecuritypolicy.yaml`, `networkpolicy.yaml`. (PriorityClass is included in `pdb-hpa-network.yaml`.)
### 4.4 Helm Chart [PARTIALLY DONE]
`charts/memobuild/` exists with `Chart.yaml`, `values.yaml`, `templates/_helpers.tpl`, `templates/deployment.yaml`. **Missing:** separate templates for RBAC, NetworkPolicy, CRD install hook. No subchart dependencies configured for bitnami/postgresql or bitnami/redis.
***
## Phase 5 — Supply Chain Security & SLSA Compliance (v0.8.0) — 4 weeks [DONE]
**Goal:** Every build artifact is signed, attested, and auditable. SLSA Level 3 achieved.
### 5.1 SLSA Provenance Generation [DONE]
`src/slsa.rs` (265 lines): `ProvenanceGenerator` — SLSA `BuildDefinition` + `RunDetails`, in-toto attestation JSON with DSSE signing/verification submodule.
### 5.2 Cosign Artifact Signing [DONE]
`src/verify.rs` (152 lines): `CosignVerifier` with Rekor entry verification, `MEMOBUILD_REQUIRE_SIGNED` policy enforcement.
### 5.3 SBOM Generation [DONE]
`src/sbom.rs` (373 lines): CycloneDX 1.5 SBOM generator with JSON/XML output, content hashing, CLI support.
### 5.4 Sigstore Policy Controller [DONE]
`deploy/k8s/policy/sigstore-policy.yaml`: `ClusterImagePolicy` enforcing valid Rekor entries for admission.
### 5.5 Audit Trail [DONE]
`src/audit.rs` (307 lines): immutable append-only audit log with SHA256 chain hashing. Records build lifecycle, cache, and cluster events.
***
## Phase 6 — gRPC Build Protocol & Remote Execution API (v0.9.0) — 5 weeks [PARTIALLY DONE]
**Goal:** Replace HTTP/JSON execution protocol with gRPC streaming. Achieve compatibility with Bazel RE API.
### 6.1 gRPC Execution Service [DONE]
`proto/memobuild/v1/execution.proto` (197 lines): `ExecutionService` (Execute, WaitExecution), `CacheService` (Get/UpdateActionResult, FindMissingBlobs, BatchRead/UpdateBlobs, GetTree).
`src/remote_exec/` full module: `reapi.rs` (397 lines), `server.rs`, `client.rs`, `worker.rs`, `worker_pool.rs`, `scheduler.rs`, `grpc_server.rs`.
### 6.2 Bazel RE API Compatibility [DONE]
`src/remote_exec/reapi.rs`: implements `google.devtools.remoteexecution.v2` proto service surface via `tonic`.
### 6.3 Build Sandboxing in Workers [PARTIALLY DONE]
`src/sandbox/` module exists with `Sandbox` trait (`prepare`, `execute`, `cleanup`), `SandboxKind::Local` and `SandboxKind::Containerd` implementations.
**Missing:** `src/sandbox/linux.rs` (landlock + Linux namespaces) and `src/sandbox/macos.rs` (sandbox-exec) as described in spec.
***
## Phase 7 — Multi-Tenancy & Enterprise (v1.0.0) — 5 weeks [NOT STARTED]
**Goal:** Org-isolated cache namespaces, quotas, billing hooks, admin portal.
### 7.1 Cache Namespace Isolation
`src/tenancy.rs`:
* Every cache key prefixed with `{org_id}/{project_id}/`. Tenants cannot read/write other tenants' artifacts.
* PostgreSQL RLS (Row-Level Security) policies enforce isolation at DB layer.
* Redis keyspace: `{org_id}:{hash}` prefix with per-org TTL policies.
* Object storage: per-org S3 prefix + optional per-org KMS key for at-rest encryption.
### 7.2 Resource Quotas
* PostgreSQL table `org_quotas`: `max_cache_bytes`, `max_concurrent_builds`, `max_artifact_ttl_days`.
* Quota enforcement middleware in `src/auth.rs`.
* K8s `ResourceQuota` + `LimitRange` per tenant namespace when using the Operator.
* Quota exceeded: HTTP 429 with `Retry-After` header + audit event.
### 7.3 Admin REST API
`src/admin/mod.rs` — routes prefixed `/admin/v1/` (requires admin token):
* `POST /admin/v1/orgs` — create org.
* `GET /admin/v1/orgs/{id}/usage` — cache bytes, build count, last active.
* `POST /admin/v1/orgs/{id}/tokens` — issue org-scoped token.
* `DELETE /admin/v1/orgs/{id}` — purge org data (GDPR right-to-erasure).
* `POST /admin/v1/gc` — trigger GC for specific org.
### 7.4 Global CDN Distribution
* `src/cdn.rs`: `CdnBackend` trait — presigned URL generation pointing to CloudFront/Fastly.
* Build client redirected to presigned URL for artifact download (avoids proxying large blobs through cluster).
* Cache-Control headers set on object storage objects for CDN edge caching.
* Multi-region: cache node in each region writes to regional bucket; cross-region replication handled by S3 CRR or GCS dual-region.
### 7.5 Developer Portal (Web UI)
`extension/` directory (JavaScript/TypeScript, already present):
* Extend existing WebSocket dashboard to full SPA using Svelte or Solid.js.
* Pages: Cluster health, Build history, Cache analytics, Org management, Token management, Audit log viewer.
* Packaged as separate OCI image `nrelab/memobuild-portal:latest`.
***
## Production SLO Targets by Phase
**P0 (v0.4.1):** MemoBuild server image ≤ 15 MB, multi-arch, signed.
**P1 (v0.5.0):** Zero plaintext inter-node traffic. All endpoints require auth.
**P2 (v0.5.1):** Artifact storage fully decoupled from local disk. GC automated.
**P3 (v0.6.0):** 100% of builds traced end-to-end. Alerting latency < 1 min.
**P4 (v0.7.0):** Cluster deployed and upgraded with zero manual YAML. PDB prevents split-brain.
**P5 (v0.8.0):** SLSA Level 3 on all `--push` builds. Every artifact signed + SBOMed.
**P6 (v0.9.0):** Bazel RE API compatible. Build tasks sandboxed. Streaming log tailing.
**P7 (v1.0.0):** Multi-tenant isolation enforced at DB+storage+K8s layers. CDN-accelerated artifact delivery. 99.95% uptime SLA.
***
## Implementation Priority Matrix
**Done or resolved:**
* ✅ Multi-stage production Dockerfile (P0.1)
* ✅ Docker Compose full-stack for dev (P0.2)
* ✅ DistributedCache layer replication (P0.4) — verified fixed
* ✅ mTLS between cluster nodes (P1.1)
* ✅ API authentication + rate limiting (P1.2)
* ✅ Object storage backend (P2.1) — S3 done, GCS stubbed
* ✅ Automated garbage collection (P2.3)
* ✅ Prometheus `/metrics` endpoint (P3.1) — Grafana dashboards still missing
* ✅ SLSA provenance + Cosign signing (P5.1–P5.2)
* ✅ gRPC execution service + RE API compatibility (P6.1–P6.2)
**Still needed:**
* Complete K8s manifests: PodSecurityPolicy, standalone NetworkPolicy, statefulset.yaml (P4.3)
* Complete Helm chart templates + subchart dependencies (P4.4)
* Linux landlock/namespace sandboxing + macOS fallback (P6.3)
* Multi-tenancy, admin API, CDN distribution (P7.1–P7.4)
* GCS backend real implementation (P2.1)
* ~~Multi-arch CI build workflow (P0.3)~~ → done in `.github/workflows/docker.yml`
***
## Key New Dependencies to Add
* `rustls` + `axum-server` with rustls — mTLS
* `rcgen` — self-signed cert generation for dev
* `tower_governor` — rate limiting
* `prometheus-client` — metrics
* `opentelemetry` + `opentelemetry-otlp` + `tracing-opentelemetry` — tracing
* `fred` — async Redis client
* `aws-sdk-s3` + `google-cloud-storage` — object storage
* `vaultrs` — Vault secret provider
* `tonic` (enable existing optional dep) — gRPC
* `landlocked` — Linux sandboxing for workers
* `in-toto` / `sigstore` — SLSA attestation
***
## Deliverables Checklist per Phase
**P0 [DONE]:** ✅ `Dockerfile.server`, `docker-compose.prod.yml`, `.env.example`, `scripts/db/init.sql`, `.github/workflows/docker.yml`.
**P1 [DONE]:** ✅ `src/tls.rs`, `src/auth.rs`, `src/secrets.rs`, `deploy/k8s/certs/certificates.yaml`, updated HTTP clients.
**P2 [PARTIAL]:** ✅ `src/storage/s3.rs`, `src/storage/local.rs`, `src/storage/mod.rs`, `src/cache_redis.rs`, `src/gc.rs`, MinIO in compose. ❌ `src/storage/gcs.rs` is stub, `stream_get` missing from trait.
**P3 [DONE]:** ✅ `src/metrics.rs`, OpenTelemetry instrumentation in `src/tracing.rs`, `monitoring/grafana/`, `monitoring/prometheus/alert_rules.yml`.
**P4 [PARTIAL]:** ✅ `src/operator/`, `deploy/k8s/crds/`, `charts/memobuild/`. ❌ Full `deploy/k8s/` manifest set (missing standalone StatefulSet, PodSecurityPolicy, standalone NetworkPolicy; PriorityClass included in pdb-hpa-network.yaml).
**P5 [DONE]:** ✅ `src/slsa.rs`, `src/sbom.rs`, `src/verify.rs`, `src/audit.rs`, `deploy/k8s/policy/sigstore-policy.yaml`.
**P6 [PARTIAL]:** ✅ `proto/memobuild/v1/execution.proto`, gRPC server + RE API, `src/sandbox/`. ❌ Linux/macOS sandbox modules.
**P7 [NOT STARTED]:** ❌ `src/tenancy.rs`, `src/admin/`, `src/cdn.rs`, extended portal SPA.