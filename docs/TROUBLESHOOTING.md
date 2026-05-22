# Troubleshooting MemoBuild

## Common Issues

### 1. Remote Cache Connection Refused
**Symptom**: `Failed to connect to remote cache: connection refused`
**Resolution**: 
- Verify the server is running (`cargo run -- server --port 3000`).
- Ensure the port is correctly matched in CLI configuration (`--remote=http://localhost:3000`).

### 2. DAG Execution Freezes
**Symptom**: Build starts but stops at `Executing layer N/M` without CPU usage.
**Resolution**:
- If using containerd, ensure `containerd` daemon is running. 
- Try running with logging output: `RUST_LOG=debug target/release/memobuild build`.

### 3. CAS Hash Mismatch
**Symptom**: API responds with `400 Bad Request: CAS Integrity Failure`
**Resolution**:
- A proxy server might be modifying your byte stream in transit.
- Verify that your remote endpoint supports gzip streams efficiently.

## Getting Help
Enable trace logs and open an issue in GitHub:
`RUST_LOG=memobuild=trace MEMOBUILD_JSON_LOGS=true cargo run -- build > logs.json`
