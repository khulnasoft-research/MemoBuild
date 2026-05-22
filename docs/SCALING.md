# MemoBuild Scaling Strategy

This document addresses memoization at scale, how to test limits, and guidance for deploying globally distributed remote cache endpoints.

## Memory Profiling

Under sustained load (100+ concurrent build streams), the primary bottleneck is usually the memory structure of `BuildGraph` objects remaining in concurrent buffers and connection sockets. To profile:

1. Enable jemalloc or mimalloc in `Cargo.toml`.
2. Run load tests via:
   ```bash
   cargo build --release
   RUST_LOG=info ./target/release/memobuild server --port 3000
   ```
3. Target 100+ concurrent requests using `wrk`:
   ```bash
   wrk -t12 -c100 -d60s http://127.0.0.1:3000/api/analytics
   ```

## Eventual Consistency Model

Currently, MemoBuild provides strict consistency checks on artifacts via CAS hash verifications before propagating them fully to metadata DB. 
For global deployments:
1. **Cache Regions:** It is optimal to share one `memobuild-server` instance per region or edge site.
2. **Metadata sync:** In eventual consistency deployments, metadata databases can be asynchronously replicated to read-replicas. By design, missing metadata causes a fallback cache miss (safe).
3. **Blob replication:** Artifacts storage should sit on a globally unified bucket (S3/GCS/R2). As blobs are content-addressable, overwrites do not corrupt data.

## Scaling Limits and Guidance

- **SQLite Bottleneck:** SQLite allows strong concurrency on reads, but writer queues will lock. We recommend maximum 50-100 concurrent artifact pushes per server node.
- **Horizontal Scaling:** Transitioning the MetadataStore from SQLite to Redis or PostgreSQL allows scaling the MemoBuild server API horizontally behind a load balancer to theoretically infinite writers.
