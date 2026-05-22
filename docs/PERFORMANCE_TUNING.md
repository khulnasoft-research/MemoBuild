# MemoBuild Performance Tuning Guide

This document provides recommendations for optimizing MemoBuild performance in different environments.

## 1. Storage Performance

MemoBuild relies heavily on I/O for hashing and artifact management.

- **Fast Cache Directory:** Set `MEMOBUILD_CACHE_DIR` to a path on an NVMe SSD or a RAM disk for maximum speed.
  ```bash
  export MEMOBUILD_CACHE_DIR=/mnt/nvme/memobuild-cache
  ```
- **Local Cache SSDs:** Ensure the local storage has high IOPS, especially during the hashing phase (detecting changes).

## 2. Hashing Optimization

MemoBuild uses **Blake3** for ultra-fast hashing.

- **Parallel Hashing:** By default, MemoBuild uses `rayon` for parallel directory walking and hashing. Ensure `RAYON_NUM_THREADS` is tuned to the number of physical cores available on your build machine.
- **Ignore Rules:** Use `.memobuildignore` or standard `.gitignore` patterns to exclude large, irrelevant directories (like `node_modules`, `target`, `.git`) from being hashed. This reduces the DAG construction time.

## 3. Caching Strategy

- **Hybrid Caching:** local cache is always faster than remote. If you are on a high-bandwidth network, enable the remote cache (`MEMOBUILD_REMOTE_URL`) to share artifacts across the team.
- **Prefetching:** MemoBuild implements smart prefetching. If you notice "waiting for artifact" delays, ensure your network latency to the remote cache server is minimal (< 20ms).
- **Garbage Collection:** Regularly run `cargo run -- --server --gc --days 7` to prevent the cache from bloating and slowing down filesystem lookups.

## 4. Sandbox Selection

- **Local Sandbox:** Lowest overhead, but least isolation. Prefer this for trusted local builds.
- **Containerd Sandbox:** Higher overhead (container startup time) but provides reproducible and isolated environments. Tune containerd to use `overlayfs` for faster layer mounting.

## 5. Remote Execution

When using distributed builds (`--remote-exec`):
- **Bandwidth:** Artifact transfer is often the bottleneck. Use a cache server in the same region/VPC as your build farm.
- **Node Count:** Parallelize nodes at the same levels of the DAG. MemoBuild will automatically dispatch parallelizable nodes concurrently.

## 6. Profiling

To profile MemoBuild:
1. Install `cargo-flamegraph`.
2. Run: `cargo flamegraph -- --file Dockerfile`
3. Analyze the output to find bottlenecks in hashing vs execution.
