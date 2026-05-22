# Quick Start Guide for MemoBuild Distributed Build System

## 1. Local Development Setup with Basic CLI Commands  
To get started with MemoBuild on your local machine, follow these steps:

### Prerequisites  
- Install **Node.js** (version >= 14)
- Install **MemoBuild CLI** by running the following command:
```bash
npm install -g memobuild-cli
```

### Basic CLI Commands  
- **Initialize a new project:**  
  ```bash
memobuild init my-project
```
- **Start the local development server:**  
  ```bash
cd my-project
memobuild start
```
- **Build the project:**  
  ```bash
memobuild build
```

## 2. Docker Compose Multi-Container Deployment  
To deploy MemoBuild using Docker Compose, follow these steps:

### Prerequisites  
- Install **Docker** and **Docker Compose**.

### Example `docker-compose.yml`  
```yaml
version: '3.8'
services:
  memobuild:
    image: memobuild:latest
    volumes:
      - .:/app
    ports:
      - "3000:3000"
    environment:
      - NODE_ENV=production
```

### Deployment Command  
Run the following command in your terminal:
```bash
docker-compose up -d
```

## 3. Kubernetes Deployment with Auto-scaling  
Deploying MemoBuild on Kubernetes allows for better scalability. Follow these steps:

### Prerequisites  
- Install **kubectl** and have access to a Kubernetes cluster.

### Example Deployment Configuration  
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: memobuild
spec:
  replicas: 3
  selector:
    matchLabels:
      app: memobuild
  template:
    metadata:
      labels:
        app: memobuild
    spec:
      containers:
      - name: memobuild
        image: memobuild:latest
        ports:
        - containerPort: 3000
---
apiVersion: autoscaling/v1
kind: HorizontalPodAutoscaler
metadata:
  name: memobuild-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: memobuild
  minReplicas: 1
  maxReplicas: 10
  targetCPUUtilizationPercentage: 50
```

### Deployment Command  
Run the following command to apply the configuration:
```bash
kubectl apply -f deployment.yaml
```

With this guide, you should be able to set up and deploy MemoBuild efficiently in multiple environments. For more detailed information, refer to the official documentation.