# Proposal

## Why

Three implementation hotspots are hard to navigate: `semantic.rs` (1,889 lines), `runtime.rs` (1,752 lines), and `codegen.rs` (1,221 lines). They also retain an older process-graph parser/emitter/executor that the `.bl` front end can no longer produce. Remove that path, then split the remaining responsibilities along real boundaries; before publication, name the surviving API for what it actually does.

## What Changes

- Remove the obsolete step-based graph parser/AST (`GraphStmt` and `graph::parse`), `Process.graph`, its validator and code emitter, plus `runtime::Definition`, `Step`, `Branch`, `Registry::new` for step definitions, and the legacy executor and its tests. The `.bl` parser already rejects implicit `run`/`return` process syntax.
- Remove old-checkpoint migration branches and fields used only for pre-current checkpoint JSON; reject unsupported old checkpoints explicitly rather than silently replaying them. Keep current-version checkpoint persistence and restart behavior.
- Group remaining semantic validation helpers by decision, named graph, and expression responsibility behind `validate`; separate remaining decision/graph emission from `generate`; separate compiled-graph orchestration from the local engine.
- **BREAKING:** rename `named_runtime` to `compiled_graph` and `runtime::Store` to `runtime::LocalStore`; use `Registry::new`/`get` for compiled graphs after removing the step-based variants. Update generated path references, project templates, documentation, binaries, and callers without compatibility aliases. Retain `named_graph_definitions()` in generated crates (its replacement `graph_definitions()` would conflict with a currently valid user task name).
- Keep currently supported `.bl` syntax, generated crate functionality, current checkpoint format, and named-graph execution behavior; no new crates/dependencies or unrelated file-size-driven rearrangement.

## Capabilities

### New Capabilities

None. This refactor removes unsupported older APIs/formats; `skip_specs: true` is set in `.openspec.yaml` because existing capability requirements describe the supported named-graph path, not the retired Rust step API or old checkpoint migration.

### Modified Capabilities

None. Existing requirements in `business-language`, `decision-models`, `process-runtime`, `distributed-execution`, and `project-build` remain unchanged.

## Impact

`src/graph.rs`, `src/compiler.rs`, `src/semantic.rs`, `src/codegen.rs`, `src/runtime.rs`, `src/named_runtime.rs`, `src/store.rs`, `src/lib.rs`, generated Rust strings, project templates, README examples, binaries, and tests. Old hand-built step APIs and old-format checkpoints cease to work; current named-graph processing and current-format checkpoint JSON stay supported. No new dependencies or CLI syntax changes.
