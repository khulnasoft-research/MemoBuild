# Monorepo Example

This example demonstrates how MemoBuild is effective in a Monorepo environment. While multi-project Dockerfiles are explicitly supported, managing dependency contexts becomes significantly easier because MemoBuild precisely hashes the subdirectories (DAG node dependencies) independently. 

For example, when you build the UI context, altering `api/api.js` has zero side-effects on the UI cache hashes.

## Usage

You can build the distinct microservices by running the Memobuild executable against their explicit subdirectories context:

### Build API:
```bash
cargo run --bin memobuild -- --file api/Dockerfile --sandbox local
```

### Build UI:
```bash
cargo run --bin memobuild -- --file ui/Dockerfile --sandbox local
```
