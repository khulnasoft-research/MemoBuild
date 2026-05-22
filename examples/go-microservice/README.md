# Go Microservice Example

This example demonstrates how MemoBuild aggressively optimizes heavily layered systems like `scratch`-based Go API deployments.

## Why MemoBuild is Better Here

The builder stage installs an entire Golang toolchain, runs the build, and emits a binary. In standard build systems, any change in `main.go` rebuilds the whole layer and outputs a new image. MemoBuild hashes the environment, pulls precisely the exact cached artifacts it needs across dependencies, and avoids redundant network pulls on the `golang:1.21-alpine` container altogether if the graph hasn't changed heavily.

## Usage

```bash
cargo run --bin memobuild -- --file Dockerfile --sandbox local
```
