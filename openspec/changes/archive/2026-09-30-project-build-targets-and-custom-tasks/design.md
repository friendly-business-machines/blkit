# Design

## Context

`blkit SOURCE.bl OUTPUT.rs` currently calls `transpile(&str)` for one self-contained source and emits Rust that depends on `blkit` and several crates. `build.rs` separately transpiles `examples/graph.bl`, which is embedded by the existing dev server, distributed API, and worker. Graph task closures use synchronous `Evaluate` and are run via `spawn_blocking`; this cannot support efficient async custom I/O as-is. See proposal.md for motivation and the delta specs for expected behavior.

## Goals / Non-Goals

**Goals:** A `.bl` author can build discovered, interdependent project files without maintaining Rust files; one selected build target uses a shared generated-library path and dependency resolution; the compiler validates crate-qualified external task calls against provider-supplied metadata; existing execution semantics apply to asynchronous custom tasks.

**Non-Goals:** Runtime-loaded plugins, new routing/gateway node kinds, exactly-once external effects, a new authentication/hosting layer, or auto-generating application-specific REST endpoints. The distributed worker remains paired with the existing separately deployed distributed API; the all-in-one server uses the existing local durable store and REST router.

## Decisions

### One project manifest, one generated crate

Introduce `blkit.toml` at the project root with exactly one `build_target`, the exact blkit toolchain version, and optional extension crate dependencies. No source or task list:

```toml
[project]
name = "orders"
blkit = "0.1.0"
build_target = "worker" # alternatively "crate" or "server"

[dependencies]
payments = "1.2"
```

Discover `.bl` files recursively under the project root in deterministic path order, excluding `.blkit/`, `target/`, and hidden directories; fail if none exist. Parse each file's mandatory namespace and version, group files by that pair, then merge their record, enum, task, decision, and process declarations before group-wide validation and generation. This lets a process use a task or type defined in another file of the same scope regardless of file order. Generate one Rust module per namespace/version group, with an aggregate `named_graph_definitions()` in the library; groups cannot implicitly reference each other. Reject duplicate declaration names in a group, including duplicate process identities, and report offending source paths. A project build validates all discovered files before reporting success, never silently omitting invalid files. Existing direct transpilation remains single-file and usable for built-in-only files.

Generate a Cargo package under `.blkit/` with a library and, only for the single selected binary target, a thin executable. Ignore its generated source/manifest but retain `.blkit/Cargo.lock` as the project's checked-in lockfile next to Cargo's generated manifest. `build_target = "crate"` exposes generated process definitions as a reusable library. `worker` packages those definitions with existing `DistributedWorker`/`PostgresStore`; it needs PostgreSQL and an independently operated REST API. `server` packages the same definitions with existing `Engine`/`Store` and local REST router, defaulting to loopback binding. Add target choices incrementally: crate first, worker second, all-in-one server third. No parallel code generators or copy of `examples/graph.bl` in the user build. Keep existing example binaries for developer workflows until they can themselves consume the project path.

Alternative: require every author to hand-write a Cargo crate/build script. Rejected as the desired project model hides generated Rust, while still exposing the generated library to Rust consumers.

### Cargo is the dependency resolver; process versions are separate

Require the manifest's exact blkit version to match the invoked CLI/compiler; use that version for the generated crate's blkit dependency. While the blkit crate is unpublished, use the CLI's local blkit source as a version-checked Cargo path dependency when available, falling back to the pinned registry dependency elsewhere; no extra user manifest field is needed. Cargo resolves declared extension requirements, maintains a lockfile in the authored project, and is invoked with locked resolution on subsequent builds (an explicit update operation may refresh it). Do not compare `.bl` `version "..."` to a toolchain version: it continues to identify process semantics and worker claims. Build-time linkage means workers never fetch code or compile source while serving work.

Alternative: write a new plugin resolver/loader and compatibility protocol. Rejected because Cargo already provides version solving, builds, and lockfiles. An exact blkit version is intentionally conservative while blkit has no independent compiler/runtime compatibility policy.

### Typed boundary for external async tasks

A `.bl` task node explicitly names its provider, for example `node payment = task payments.charge(input)`. The `payments` dependency ships a `blkit-tasks.toml` descriptor with exported task names, Rust function paths, and `.bl` input/output types (for example `charge: Order -> Receipt`). After Cargo resolves the project's dependencies, blkit reads the descriptors from the resolved packages before generating Rust; this is an explicit extension contract, not introspection of arbitrary Rust signatures. It resolves `Order` and `Receipt` in the caller's namespace/version declaration scope, type-checks the argument and downstream uses, and rejects missing/invalid metadata, unknown types, and undeclared providers. The `.bl` graph syntax gains a crate-qualified task reference, not new control-flow node kinds; no project task list or standalone `.bl` external-task declaration is needed.

The extension crate exports an async callable accepting and returning `serde_json::Value` with `Result<Value, String>` (or an equivalent boxed-future callable), so it does not depend on generated domain types. Generated glue checks the function path and async interface when compiling the generated crate, serializes a validated typed argument, and checks/deserializes the result against the advertised `.bl` output type before checkpointing. A descriptor may claim a type the function violates at runtime: malformed values and task errors become execution failures, never successful checkpoint values. Validate dependency aliases and exported paths as identifiers before inserting them into generated Rust.

Add an async task variant to the compiled graph/executor instead of changing expression and routing evaluation (which remain synchronous). Spawn/await custom futures under the existing in-flight limit, alongside current blocking built-in tasks; wire cooperative cancellation so later completions cannot commit after accepted cancellation or deadline. Existing synchronous graph APIs retain built-in behavior; if an async graph is invoked through a synchronous-only execution helper, return a clear unsupported-mode error instead of blocking an executor. Local and distributed execution paths both pass through the shared task scheduler. Avoid dynamic Rust ABI loading or crate-side registration/inventory.

Alternative: block on futures in `spawn_blocking`. Rejected because long-lived I/O would occupy blocking threads and complicate cancellation; async task scheduling already exists at the executor boundary.

## Risks / Trade-offs

- [Provider-supplied task metadata can disagree with actual JSON payloads] -> Verify callable shape at Rust compile time and validate values at the generated boundary, treating bad outputs as task failures. No false claim of Rust-level domain-type checking across the JSON boundary.
- [External side effects can repeat after lost claims] -> Document at-least-once semantics and require idempotency by task authors; keep checkpoint/lease fencing before successor dispatch.
- [Generated crate dependencies drift from the CLI] -> Reject mismatched blkit versions early and persist Cargo's lockfile in the author project.
- [Multiple sources can collide on declaration names or process identities] -> Validate each merged namespace/version group before generation; reject duplicate declarations with both source paths. Keep different groups isolated.
- [Build artifacts are generated rather than authored] -> Keep generated Rust in an ignored directory and preserve `blkit SOURCE.bl OUTPUT.rs` for existing integrations.

## Migration Plan

No forced migration: existing `.bl` files and direct transpile commands continue to work. A user may add `blkit.toml` and select `build_target = "crate"` first, switching that one value to `worker` or `server` as those packaging choices become available. Deploy a new `.bl` process version when its behavior changes; retain workers for old queued versions during distributed rollout. Rollback by rebuilding the last locked project version, not by remapping queued instances to a different process version.
