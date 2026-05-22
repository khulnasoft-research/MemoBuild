# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-02-22

### Added
- **Reproducible Builds:** Implemented bit-for-bit reproducibility for OCI image exports using `--reproducible` flag.
- **CI/CD Pipeline:** Added GitHub Actions workflow for automated testing, linting, and multi-platform builds.
- **Performance Tuning Guide:** New documentation in `docs/PERFORMANCE_TUNING.md`.
- **Architecture Decision Records (ADRs):** Documented core decisions regarding the extension system and API schema.
- **Multi-Language Examples:** Added `examples/` for Python multi-stage builds, Go microservices, and Node.js monorepos.
- **Centralized Constants:** Extracted magic numbers into `src/constants.rs`.
- **Unconditional Module Structure:** Removed feature gates for the `server` module to improve testability.

### Fixed
- Fixed variable ownership issues in `src/executor.rs` related to `clippy::too_many_arguments`.
- Improved error handling in OCI registry client (replaced fragile `.unwrap()` calls).
- Normalized tarball timestamps and metadata for deterministic hashing.

### Changed
- Refactored `src/main.rs` to allow starting the server without explicit feature flags.
- Updated `Cargo.toml` to make `axum`, `tower-http`, and `rusqlite` core dependencies.

## [0.1.3] - 2026-02-17

### Added
- Initial support for Remote Cache server.
- Basic Dockerfile parsing and DAG execution.
- Blake3 internal hashing engine.
