# Design

## Context

See proposal.md for motivation and the three delta specs for behavior. `src/compiler.rs`, `src/graph.rs`, `src/semantic.rs`, and `src/codegen.rs` currently parse structured `run`/gateway/`join`/`return` process bodies. `src/runtime.rs` executes in memory with a per-instance lock and a shared task semaphore; `src/store.rs` persists only instance-level status/input/outcome in a local Turso database. `src/bin/blkit-dev.rs` includes generated Rust from `build.rs` at build time; `src/server.rs` exposes start/status/cancel. There is no resumable node state or multi-process ownership today.

## Goals / Non-Goals

**Goals:** Share one deterministic, checkpointable graph executor between local and distributed modes; maintain exclusive, lease-fenced ownership of each instance while letting one worker execute many instances; preserve committed parallel outputs across errors, crashes, and restarts. Migrate examples/tests and document the intentional process-syntax break.

**Non-Goals:** Exactly-once task side effects, dynamic worker compilation, a broker, automatic conversion of old `.bl` source, cyclic graphs, scoped BPMN event catching/compensation, remote external tasks, or verifying that two binaries advertising the same declared process identity have identical compiled graph contents.

## Decisions

### Compile an explicit directed graph from `.bl`

Replace implicit sequencing with named `start`, task, AND/OR/XOR split and matching join, and `end`/`error`/`cancel`/`terminate` nodes, plus explicit ordered `link` declarations. Keep task bodies' `return` as the way tasks produce typed outputs; reject process-level `return`. One illustrative XOR form (the grammar is chosen here, not inferred by the runtime):

```bl
process decide(input: Order) -> Decision:
  retry max_retries 3 retry_for "10m" retry_delay "1s" backoff exponential
  node start = start
  node amount = task total(input)
  node route = xor_split
  node high = task review(input)
  node low = task approve(input)
  node chosen = xor_join(route)
  node done = end
  link start -> amount
  link amount -> route
  link route -> high when amount > 1000
  link route -> low else
  link high -> chosen(high)
  link low -> chosen(low)
  link chosen -> done(chosen)
```

Links carry control and, where needed, typed data. `link src -> dst(expr)` supplies a typed value to a join or normal end. AND split outgoing links identify record-field labels; its paired join builds that record. XOR takes the first matching outgoing link; OR takes all matches and collects selected join values in outgoing split-link order. XOR/OR require an `else` link. Compiler validation enforces one start, unique nodes, finite acyclic reachable routes, matched split/join regions for normal flows, type/availability of expressions on every route, and a terminal on every route; exceptional terminal routes may bypass a join because they stop the entire instance. Normal `end` receives exactly the declared process output type; named `error` needs no payload and its node name is the business outcome. `cancel` and `terminate` are distinct process-wide outcomes with no scoped BPMN behavior. A normal `end` cannot complete while activated branches still require a join.

Alternative: append end statements to the existing structured syntax. Rejected because the agreed source model is a graph of named nodes and explicit links. Arbitrary cyclic/unstructured joins are deferred because their token and checkpoint semantics are not defined.

### Persist a process-state checkpoint, not a task-owned route

The process executor owns current node activations, selected split links, completed node IDs and typed outputs, pending/in-flight branches, and join progress. Task callbacks produce values only. Serialize completions for one instance; in one short store transaction, validate ownership and active status, record task output, evaluate resulting gateway transitions, and persist the next process state before successors are dispatched. Independent parallel tasks can still run concurrently. If B commits before C's failure, B's output survives; a later completion after failure/cancellation cannot advance. A new owner reconstructs ready work from the checkpoint, rerunning only uncommitted work. Preserve a shared execution algorithm and a narrow set of storage operations with Turso and PostgreSQL implementations; avoid duplicating graph semantics in the two modes.

Alternative: checkpoint only the last task ID or replay the full graph. Rejected because OR/AND joins, already completed parallel tasks, and branch data would be lost or rerun.

### Retry only execution failures, with durable per-process policy

Compile the `.bl` declaration into `max_retries` (additional attempts), `retry_for` (positive duration), `retry_delay` (positive minimum), and exponential backoff. Default is no retry. On first execution failure, persist its time; each later eligible retry is scheduled no sooner than `retry_delay * 2^(retry_number-1)` after failure, and is allowed only if both the additional-attempt count and the first-failure time window permit it. If the next scheduled attempt falls beyond the window, mark terminally failed rather than queueing an impossible retry. Persist attempt count, first-failure time, next eligibility, and last failure reason. A failed attempt enters retry-waiting while eligible; it is not a terminally failed instance. Errors from tasks or graph evaluation and expired owner leases share this path. Stop advancement and signal in-flight siblings on a live execution error. Reaching a named `error` is an intentional business outcome, not an execution exception; `end`, `error`, `cancel`, and `terminate` do not retry. Both process-wide cancel and terminate signal in-flight siblings, stop advancement, and record distinct outcomes.

Alternative: restart from process input or retry business-error events. Rejected because the first repeats committed work and the second confuses modeled domain control flow with execution failure.

### Use PostgreSQL instances as both durable queue and state store

Distributed REST service and worker processes use the same PostgreSQL database. The instances table carries process identity, input, status, checkpoint, retry metadata, next eligible time, owner ID, lease expiration, and a monotonically increasing claim generation. A worker process has a unique runtime ID, capability records for its linked definitions, a heartbeat, and a draining flag. Claim transactions use `FOR UPDATE SKIP LOCKED` (or equivalent atomic row claiming) restricted to queued eligible exact identities and worker capacity; increment generation and assign owner/lease without deleting the instance row. Use database time for deadlines. Renew claim leases while instances run, separately from worker heartbeat. Worker-owned checkpoint, renewal, and terminal writes must test owner, unexpired lease, generation, and current active/cancellation state in the same transaction; a stale worker cannot commit after takeover.

A periodic reconciler in the distributed REST service handles expired claims and records an attempt failure plus a scheduled retry or terminal failure, even when the lost worker never returns. A live worker's claim loop can also reconcile eligible expired work, so work progresses after an API outage; conditional updates make repeated reconciliation safe. Polling is sufficient for first delivery; no broker or `LISTEN/NOTIFY` is required. A queued instance without a capable process version remains queued and visible. The worker binary contains generated Rust built elsewhere, advertises its embedded identities on startup, and runs multiple instances under a configurable local in-flight task limit. A drain request stops new claims while keeping current claim leases renewed until those attempts finish or release for retry; queued retries need another capable worker. Workers advertise only definitions linked into their binaries. Compatibility fingerprints for same declared process identity are deliberately deferred; deployment must not mix changed definitions under one identity.

Alternative: a separate durable broker or one RPC per claim/checkpoint through a coordinator. Rejected for this stage because PostgreSQL already provides atomic ownership and durable state, and workers were explicitly approved to access it directly. A long-held database row lock for the duration of a process is also rejected: use short claim/checkpoint transactions and expiring leases instead.

### Preserve REST control ordering and local-mode parity

Keep start/status/cancel routes. Distributed start validates against compiled process definitions available to its API binary and inserts the queued instance; workers never transpile source at runtime. The distributed REST listener binds to loopback by default; remote access must use an operator-managed authenticated ingress rather than exposing an unauthenticated listener. Distributed cancellation commits a cancelling/cancelled state before further worker checkpoint/dispatch; the worker observes it through store checks/polling and signals its in-flight task hooks. Status includes retry-waiting, business-error node identity, terminal reason, and attempt metadata. Use the same graph executor in the Turso dev server: a local in-progress instance interrupted by restart is treated as an execution failure, restored from its persisted checkpoint and scheduled for retry if allowed, otherwise failed. Pending work remains pending; accepted cancellation cannot be overwritten. Migrate `build.rs`'s bundled example and tests to the new source syntax; keep `blkit` as an ahead-of-time transpiler, not a runtime parser.

Alternative: retain local fail-on-restart semantics or keep old process `return` as an alias. Rejected because the same `.bl` process must behave consistently in development and deployment, and processes must explicitly flow into end events.

### Exercise distributed behavior against a real PostgreSQL container

Use the Rust `testcontainers-modules` PostgreSQL module as a dev dependency for integration tests. Pin the PostgreSQL image tag, isolate test data between cases, and let Testcontainers handle startup, readiness, the connection URL, and cleanup. Exercise competing claims and two separate worker processes (including killing one mid-instance) against the same container; mocks cannot prove lease or transaction behavior. Check early that Testcontainers can launch the image through this devcontainer's Docker-compatible Podman socket. Document the test command and run the same integration tests in CI.

Alternative: maintain a Docker shell harness and manually poll for database readiness/clean up containers. Rejected because the Rust module already owns that test lifecycle.

## Risks / Trade-offs

- A worker may finish an in-flight task after its lease expires -> fence database writes; task code and eventual external effects are at-least-once, not exactly-once. Require idempotency for future side-effecting tasks.
- Too-short leases can cause false owner loss; too-long leases delay retry -> configure lease/heartbeat timing, use database time, and test paused workers and late results.
- Concurrent completions can race with failures or cancellations -> serialize process-state transitions and persist each committed outcome before dispatch.
- A database outage prevents claims/checkpoints -> stop advancement rather than running uncheckpointed successors; resume only after storage recovers and ownership is re-established.
- New source grammar and status fields break older `.bl` processes/consumers -> migrate examples, document changed statuses and source format, keep REST paths unchanged.
- Workers for old versions can leave work stranded -> expose queued status, drain deliberately, and retain a capable worker until its version's backlog is resolved.

## Migration Plan

1. Change the compiler, generated graph, runtime executor, and bundled `.bl` examples together; migrate all existing process declarations to named nodes/links/end events. Task `return` remains valid. Reject legacy process bodies with a clear diagnostic.
2. Extend the Turso schema to store checkpoints/retry state while preserving existing terminal records. Pre-change active records without a checkpoint are marked interrupted/failed rather than guessed/replayed; new instances use the shared semantics.
3. Add PostgreSQL schema and separate distributed REST/worker launch modes. Build/link the same generated process definitions into the binaries, start PostgreSQL, launch the API and one or more workers, and then migrate traffic from local-only mode. Do not import existing local instances automatically.
4. Roll out new versions with old-version workers still running until owned and queued work finishes; drain only when appropriate. Rollback requires stopping new claims and retaining workers capable of checkpointed old-version processes. No same-identity graph compatibility detection is provided in this stage.
