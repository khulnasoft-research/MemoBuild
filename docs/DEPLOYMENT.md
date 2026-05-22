# MemoBuild Deployment

Deploying the MemoBuild Remote Cache Server enables team-wide caching benefits, severely dropping CI/CD redundant runtimes.

## Standalone Deployment (Linux/macOS)

For single servers or simple build caches:
```bash
# Setup cache directory
mkdir -p /var/lib/memobuild-server

# Run the server on port 3000
MEMOBUILD_JSON_LOGS=true \
RUST_LOG=memobuild=info \
cargo run --release --bin memobuild -- server --port 3000 --data-dir /var/lib/memobuild-server
```

## Docker Compose
For testing locally or running via standard docker stacks:

```yaml
version: "3.9"
services:
  memobuild-server:
    image: nrelab/memobuild-server:latest
    ports:
      - "3000:3000"
    environment:
      - RUST_LOG=info
      - MEMOBUILD_JSON_LOGS=true
    volumes:
      - memobuild-data:/data
    command: ["server", "--port", "3000", "--data-dir", "/data"]

volumes:
  memobuild-data:
```

## Kubernetes Deployment (Helm/Manifests)

For production, we recommend deploying with Kubernetes scaling behind an Ingress Controller. The database currently uses SQLite, so it must be run on a StatefulSet or backed by a PVC if sharing a single instance. In the future, a shared metadata layer using Redis or Postgres will arrive with eventual consistency improvements.

```yaml
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: memobuild-server
spec:
  serviceName: memobuild-service
  replicas: 1
  selector:
    matchLabels:
      app: memobuild
  template:
    metadata:
      labels:
        app: memobuild
    spec:
      containers:
      - name: server
        image: nrelab/memobuild-server:latest
        args: ["server", "--port", "3000", "--data-dir", "/data"]
        ports:
        - containerPort: 3000
        volumeMounts:
        - name: memobuild-pvc
          mountPath: /data
  volumeClaimTemplates:
  - metadata:
      name: memobuild-pvc
    spec:
      accessModes: ["ReadWriteOnce"]
      resources:
        requests:
          storage: 100Gi
---
apiVersion: v1
kind: Service
metadata:
  name: memobuild-service
spec:
  ports:
  - port: 80
    targetPort: 3000
  selector:
    app: memobuild
```

## Maintenance

### Garbage Collection
Invoke GC manually by hitting the `/gc?days=<days>` endpoint.
```bash
curl -X POST http://memobuild-server:3000/gc?days=14
```
Alternatively, configure a cron job to keep disk size below the specified max-capacity limits.
