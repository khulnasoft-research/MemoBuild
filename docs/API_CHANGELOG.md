# API Changelog

This document tracks changes to the MemoBuild Remote Cache API and Data Structures.

## Breaking Change Policy

MemoBuild strives to maintain backwards compatibility whenever possible. Future breaking changes will:
1. Bump the API version (`X-MemoBuild-API-Version`) to a new major number.
2. Be documented in this file at least one minor release beforehand.
3. Keep the old endpoint routing logic alive until 1 major release fully phases it out.

## v1.0 (Current)
*Introduced in MemoBuild v0.2.0*

**Features:**
- Implements `X-MemoBuild-API-Version` header requirement.
- **`HEAD /cache/:hash`**: Checks artifact existence.
- **`GET /cache/:hash`**: Downloads gzip compressed blob.
- **`PUT /cache/:hash`**: Uploads compressed blob (Strict CAS hashing verification enforced).
- **`HEAD/GET/PUT /cache/layer/:hash`**: Layer specific endpoints.
- **`GET/POST /cache/node/:hash/layers`**: Layer registration mapping endpoints.
- **`POST /gc`**: Triggers metadata garbage collection and orphaned blob sweeping.
- **`POST /analytics` & `/build-event`**: Metric tracking endpoints.
- **`GET/POST /dag`**: DAG synchronization state syncing.

**Deprecations:**
- None.

**Breaking Changes:**
- None. 
