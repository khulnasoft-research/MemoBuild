# MemoBuild Code Style & Guidelines

This document outlines the coding standards, code style enforcement, and clippy overrides for the MemoBuild project. Adhering to these guidelines ensures consistency, maintainability, and safety across the codebase.

## 1. Error Handling

We follow a consistent and robust error handling strategy using `thiserror` and `anyhow`.

- **Libraries (Modules):** Use specific error types powered by `thiserror`. All internal library modules must return `Result<T, MemoBuildError>`. See `src/error.rs` for definitions.
- **Applications (Executables):** Use `anyhow::Result` in top-level app logic (e.g., `main.rs`, CLI entrypoints) for seamless error propagation and context enrichment.
- **`unwrap()` / `expect()`:** These are heavily discouraged in production code. Use them only when:
  - You can unconditionally guarantee the operation won't fail (e.g., regex compilation on a static string).
  - You are writing tests (where panicking on failure is expected).
  - All legitimate uses of `expect()` MUST include a descriptive message explaining *why* the panic is unreachable.

## 2. Unconditional Module Structure

To ensure maximum test coverage out of the box, we structure modules unconditionally.

- **Bad:** `#[cfg(feature = "server")] pub mod server;`
  This prevents `server` module tests from being compiled or analyzed unless the feature flag is passed.
- **Good:** Compile the module itself unconditionally, but feature-gate the *dependencies* inside the module or provide mock implementations. This ensures tests within the module boundaries are always available to the compiler.

## 3. Magic Numbers

Magic numbers (hardcoded constants within inline logic) are banned.

- All numeric constants should be extracted to `const` declarations at the top of the file or in a shared `constants` module.
- Add comments explaining where the number comes from (e.g., `/// 5 seconds default timeout for remote cache` ).

## 4. Clippy Allowlist & Justification

We run `cargo clippy` with a strict configuration. However, certain lints are allowed globally based on the project's specific constraints. The following lints are permitted:

- `clippy::too_many_arguments`: Allowed for core executor functions (like `execute_node_logic`) where passing multiple dependencies is preferred over creating intermediate context structs, avoiding unnecessary allocations or complex lifetimes.
- `clippy::module_inception`: Allowed when a module has the same name as its parent directory if the structure demands it (e.g., `server/server.rs`).

*To add exceptions locally, justify them with a comment:*
```rust
// ALLOW: We need a large buffer for OCI layer processing to improve I/O throughput.
#[allow(clippy::large_stack_arrays)]
```

## 5. Formatting

Formatting is strictly enforced via `rustfmt`.

- Run `cargo fmt` before every commit.
- CI will fail on unformatted code (`cargo fmt --all -- --check`).
