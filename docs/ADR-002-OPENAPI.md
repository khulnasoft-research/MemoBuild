# Architecture Decision Record (ADR): OpenAPI Specs for MemoBuild Cache server

## Context
MemoBuild runs an Axum-based web server handling HTTP requests for the remote cache and dashboard telemetry. Generating formal API documentation ensures backwards compatibility, simplifies tooling integration, and sets standard client contracts.

### Current State
Presently, REST endpoints and WebSockets are manually wired in `src/server/mod.rs` without schema documentation (like OpenAPI or Swagger). Since our API surface heavily relies on HTTP semantics representing Cache Addressable Storage rules (e.g. `HEAD /cache/:hash`, `PUT /cache/layer/:hash` with Blake3 hashes), the structure is very stable but unrecorded.

## Decision
Due to the constraints of v0.2.0, we will abstain from implementing fully integrated automated OpenAPI generation (e.g., using `utoipa` which requires extensively annotating the inner Axum handlers). 

Instead, we commit to manual API documentation tracking within Markdown until the API reaches its 1.0 maturity milestone. 

## Schema Documentation

### The MemoBuild Remote API v1.0
- **`HEAD /cache/:hash`**: Checks if an artifact layer hash exists. Returns 200 OK or 404 NOT FOUND.
- **`GET /cache/:hash`**: Downloads the binary artifact for a given hash. Returns 200 OK or 404 NOT FOUND.
- **`PUT /cache/:hash`**: Uploads a binary artifact payload. The remote server executes server-side Blake-3 verification matching the hash. Returns 201 CREATED or 400 BAD REQUEST.
- **`HEAD /cache/layer/:hash`**: (Analogous functions for granular `layer` payloads.)
- **`POST /gc`**: Garbage collects artifacts older than a querysting query parameter `days` (e.g., `?days=7`). Retrieves 200 OK containing cleanup metadata.
- **`POST /build-event`**: Ingests `BuildEvent` JSON objects representing execution telemetry updates (e.g., `NodeStarted`, `NodeCompleted`, `BuildCompleted`). Used structurally for global dashboard updates.
