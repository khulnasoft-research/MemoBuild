# Architecture Decision Record (ADR): Docker Extension System

## Context
MemoBuild requires the ability to interpret non-standard Dockerfile derivations to allow users to build highly specialized graphs (e.g., executing specialized hooks, parallelizing certain COPY operations dynamically). The `src/docker/extensions/` module was originally prototyped to handle this through custom instructions like `RUN_EXTEND`, `COPY_EXTEND`, and `HOOK`.

### Current State
As of v0.2.0, the extension system is hardcoded into `src/docker/extensions/parser.rs` and mapped directly into the core `NodeKind` enum. 
- **Is the extension API stable?** No. It is highly prototypical.
- **Can users write custom extensions?** Not currently. It requires recompiling the Rust core and modifying the `NodeKind` enum.
- **Is there a Registry for community extensions?** No. 
- **Documentation for extension development?** Not available, as user-defined extensions are not yet implemented.

## Decision
We are deprecating the hardcoded `RUN_EXTEND`/`COPY_EXTEND` commands in favor of a future WebAssembly (Wasm) plugin architecture (slated for v0.4.0). 

By embedding a Wasm runtime (e.g., Wasmtime), users will be able to supply compiled `extensions.wasm` modules that MemoBuild can safely invoke to parse unrecognized Dockerfile directives and translate them into standard DAG sequences without needing to recompile the core binary.

## Consequences
- We will not spend further effort documenting the current hardcoded extension system.
- The immediate focus will be on hardening standard OCI layer parsing and caching operations.
- The community extension registry will be deferred until the Wasm plugin system is implemented.
