# Proposal

## Why

Today blkit writes a Rust source file, and its example binaries compile a hardcoded `.bl` file. `.bl` authors need an ordinary project workflow that builds their processes without hand-maintaining a Rust application, while retaining a crate option for Rust integrations.

## What Changes

- Introduce a blkit project that discovers its `.bl` files without listing them in configuration. Sources sharing a namespace and process version form one declaration scope, so processes may use types, tasks, and decisions from other files in that scope. A single `build_target` selects `crate`, `worker`, or all-in-one `server`; all three use the same generated Rust crate build path, added in that order.
- Allow the project to declare Rust crate dependencies and versions supplying asynchronous custom task nodes. `.bl` calls name the providing crate; each crate ships discoverable task signatures, so projects do not repeat source lists or task definitions in `blkit.toml`. Resolve/lock dependencies with Cargo and link them at build time, not in a live worker.
- Extend validation and generated graphs to call custom tasks with checked input/output types. Preserve blkit-owned routing, checkpointing, retries, and cooperative cancellation; external effects are at least once, not exactly once.
- Keep `.bl` process identity (`namespace` and `version`) distinct from blkit/toolchain and extension crate versions. Retain the existing direct `blkit SOURCE.bl OUTPUT.rs` workflow for compatibility.

## Capabilities

### New Capabilities

- `project-build`: User-facing multi-file discovery and grouping, version/dependency resolution, and one selected crate/worker/server build target.

### Modified Capabilities

- `business-language`: Resolve source-defined types, tasks, and decisions across project files with matching namespace and process version, and validate crate-qualified external task calls against provider-supplied signatures.
- `process-runtime`: Execute asynchronous custom tasks without blocking task capacity, under existing checkpoint/retry/cancellation semantics.

## Impact

Project CLI and build orchestration, code generation and semantic validation, graph task execution, and packaging of existing local-server/distributed-worker entrypoints. Cargo resolves Rust dependencies; no dynamic plugin loader or new deployment broker is introduced.
