# Proposal

## Why

Generated Rust projects currently depend on the same `blkit` package that contains the transpiler and unconditional CLI, HTTP, PostgreSQL, local-store, and telemetry dependencies. This makes a generated library compile far more than it needs. Separate compilation from execution and select runtime dependencies by role while allowing one project to deploy an API and worker together or independently.

## What Changes

- Introduce a Cargo workspace with `blkit-transpiler` (compiler, project generation, and a binary still named `blkit`) and `blkit-core` (generated-code helpers, graph/runtime APIs, and target-specific services). Keep compatible CLI commands and `blkit.toml`'s `blkit` version field.
- **BREAKING:** generated source and manifests depend on/import `blkit-core` (`blkit_core` in Rust) rather than the `blkit` library; Rust consumers of the old `blkit` library must update imports and dependencies. Pin the generated core version to the compatible transpiler release.
- Provide additive `api-server`, `worker`, `local-persistence`, and `remote-persistence` core features: HTTP and execution are independent roles, worker execution supports both backends, and both persistence features may be enabled. Keep `crate` lean. Add `api-only` (remote, source-free permitted), `api-worker` (one binary, explicitly local or remote), `api-worker-split` (separate remote API and worker binaries), and `worker-only` (remote); retain existing `server` and `worker` targets as compatibility aliases. Avoid unnecessary direct dependencies and feature-unification bloat in independently built outputs.
- **BREAKING:** the local API+worker binary persists admitted work to its existing local store and an in-process worker polls/claims it under the concurrency limit, instead of starting execution directly from the HTTP request. Local input validation remains immediate; pending/running instances still recover after restart.
- **BREAKING:** distributed API-only binaries no longer link executable process definitions. At start, the API checks PostgreSQL for a recent, non-draining worker advertising the exact namespace/version/process; it accepts a request only when one is available. After acceptance, the worker validates typed input and initializes the checkpoint, so malformed typed input becomes an inspectable failed instance rather than an immediate HTTP input error. A worker disappearing after admission does not invalidate an accepted request.
- Keep existing local-server synchronous input validation, process semantics, and example binaries working without a core-to-transpiler dependency cycle; do not introduce standalone API-server or worker library packages.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `project-build`: generated crate dependencies and source imports become core-only; source-free remote `api-only`, combined local/remote `api-worker`, split remote `api-worker-split`, and remote `worker-only` targets have role-specific dependencies and compatible release versioning.
- `process-runtime`: distributed API admission consults advertised live worker capabilities, while typed input validation and checkpoint initialization move to remote workers; a local in-process worker polls the durable queue while local validation remains immediate.

## Impact

Cargo workspace/manifests, `src/main.rs`, `src/compiler.rs`, `src/project.rs`, `src/codegen/`, runtime modules and feature gates, `build.rs`, generated entrypoint templates, tests that compile generated projects, CLI fixtures, and README instructions. The in-progress `add-temporal-expressions` change touches expression/type/codegen and may add generated runtime helpers; reconcile that change before moving shared source files.
