# Design

## Context

The compiler accepts one `.bl` file and emits synchronous process functions, but no task graph. A process map must be authored in `.bl`, checked before code generation, and supplied to the runtime as a compiled definition. Existing one-body `.bl` processes and their generated Rust remain valid. See proposal.md for motivation and the language/runtime delta specs for behavior.

## Goals / Non-Goals

**Goals:**
- Make `.bl` the sole source of graph topology, conditions, and typed task data flow.
- Execute compiled AND/OR/XOR routing and cooperative cancellation with clear event ordering.
- Persist instance identity, input, and terminal outcomes across server restarts.

**Non-Goals:**
- External Rust/Python task code, loops, arbitrary BPMN event types, distributed coordination, or client interaction.
- Automatic resume after a crash, exactly-once external side effects, or forced interruption of synchronous `.bl` logic.

## Decisions

### Compile source-defined tasks and structured gateways
Add `.bl` task bodies using the existing typed expression/branch/return core and process-map syntax for named task nodes, typed links, and explicit AND/OR/XOR splits and joins. Preserve the existing `process ...:` body form as a one-node process. Graph-mode processes are finite, acyclic, and have one typed output. Gateways are structured and paired (split/join) to keep inclusive-join activation decidable without an arbitrary BPMN graph analyzer. The compiler checks names, reachability, cycles, typed connections, condition visibility, and output type on every route before emitting any Rust.

At an AND split all branches activate; an XOR split takes the first true condition in source order, otherwise a required fallback; an OR split takes every true condition, otherwise a required fallback. Conditions are `Bool` expressions referring to the process input and outputs completed on every path to that gateway; referring to an output that may be absent is rejected. AND joins wait for all branches; XOR joins take the selected branch; OR joins wait only for the branches selected on that instance. For typed values leaving a join: AND combines branch results into a declared record with matching branch fields, XOR merges a common result type, and OR gathers compatible results into `List<T>` in branch-declaration order. Tasks downstream consume the resulting typed value, not unchecked nullable/Any results. Nested structured gateways are allowed where the same visibility and type rules hold.

Alternative: the runtime constructs a Rust-side graph from registered functions. Rejected: it duplicates the process map outside `.bl` and undermines the point of writing business processes in source files. Full BPMN graph semantics and arbitrary cycles are deferred because they need additional token/join and persistence rules.

### Emit an executable graph; keep existing generated modules standalone
For graph-mode `.bl` processes, codegen emits task functions, typed gateway predicates/data adapters, and a static process definition describing nodes, edges, branch order, split/join pairing, namespace, version, and process name. The runtime registers and executes that definition; it does not invent connections. Conversion to/from JSON happens at the server boundary via generated typed adapters, so invalid input fails before an instance starts. Existing single-body processes continue to generate callable Rust without a new mandatory runtime dependency; graph-mode output can depend on the runtime crate. Compile a sample `.bl` graph into the dev-server binary as a build step rather than parsing `.bl` on every request.

Alternative: interpret `.bl` source in the server. Rejected because the platform aims to compile business logic into a binary.

### Serialize per-instance transitions; bound dispatch across instances
An instance coordinator owns activation state for each compiled node and a per-instance async lock. Gateway activation, task dispatch/registration, completion, failure, and cancellation pass through that lock. A global semaphore (default 32 permits, configurable at server start) bounds active tasks across all instances; before dispatch, acquire capacity and recheck instance state under the lock. Keep locks and database transactions short; never hold either while executing a task or invoking its cancellation hook. Different instances progress concurrently, and independent activated task nodes use spare capacity.

Alternative: one thread per task or a global instance lock. Rejected because bursts exhaust threads or serialize unrelated instances.

### Cooperative cancellation and stable terminal status
After persisting a cancelling status under the instance lock, stop gateway/token advancement and snapshot every in-flight task for cancellation calls. Issue each call without holding the lock; once requests are issued, mark cancelled. Late completions cannot dispatch successors or overwrite cancellation. Failure stops advancement and requests cancellation of in-flight siblings. An in-progress synchronous `.bl` task cannot be forcibly interrupted mid-function: its cancellation hook signals the request, and its eventual result is ignored. Cancelled means advancement stopped, not that all external effects were rolled back.

Alternative: drop futures and declare external work cancelled. Rejected because abort does not necessarily invoke cleanup, and waiting indefinitely for acknowledgement would hang cancellation.

### Turso in-process state; no replay in this iteration
Use the Rust `turso` crate in local, file-backed mode (no Turso Cloud or database server) to persist unique instance ID, namespace/version/name, canonical JSON input, status, output or error, and timestamps. Use explicit transactions for related state changes and commit status transitions before acknowledging them. On startup, mark nonterminal instances failed/interrupted rather than replaying them: graph activation stays in memory and a crashed task could have performed work without recording completion. Test reopen and abrupt-exit recovery of acknowledged writes on the pinned crate version before relying on its durability.

Alternative: mature SQLite via `rusqlite` has a longer track record, but Turso provides the requested native Rust in-process engine. In-memory-only state loses inspection after restart; automatic replay risks duplicating effects without idempotency and durable checkpoints.

### Separate dev-server binary with a small JSON API
Keep `blkit SOURCE.bl OUTPUT.rs` unchanged. A separate binary registers the compiled `.bl` graph, opens a configured local Turso database file, and serves `POST /processes/{namespace}/{version}/{name}/instances`, `GET /instances/{id}`, and `POST /instances/{id}/cancel`. Use an established Rust HTTP/JSON stack, the `turso` crate, and collision-resistant IDs. Bind to loopback by default; expose a different interface only by explicit opt-in, without claiming production authentication.

Alternative: combine the compiler CLI with an on-the-fly source interpreter/server or hand-write HTTP parsing. Rejected to preserve compile-to-binary behavior and avoid protocol boilerplate.

## Risks / Trade-offs

- Structured, acyclic graphs exclude BPMN loops and unstructured joins -> Reject them with clear compiler diagnostics; add only with defined token semantics in a later change.
- Conditions on an output from an unselected branch are invalid -> Compile-time availability checks avoid null or dynamic runtime values.
- Synchronous `.bl` work may still run after cancellation -> Stop token advancement and ignore late output; document cooperative limits.
- Crash during work can leave effects even though status becomes failed -> Do not auto-replay without idempotency/checkpoint design.
- Turso is a newer SQLite-compatible engine with incomplete compatibility edges -> Pin and test the exact crate version, keep queries simple, and test concurrent writes and abrupt-exit recovery on the target filesystem.
- Exposing the dev server without authentication is unsafe -> Bind to loopback by default.

## Migration Plan

No runtime data exists to migrate. Keep existing single-body `.bl` programs and compiler CLI output compatible. Add graph-mode syntax and the separate binary with a new example. Rollback removes the new binary/compiler graph mode; old programs still compile.
