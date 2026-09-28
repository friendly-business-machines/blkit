# Design

## Context

See [proposal.md](proposal.md). `src/compiler.rs` parses top-level declarations, `src/graph.rs` parses process nodes, `src/semantic.rs` currently rejects all cycles and calculates scopes by traversing a DAG, and `src/codegen.rs` emits typed Rust closures. `src/named_runtime.rs` checkpoints a node-name-keyed result map, ready activations, and split/join progress. `src/runtime.rs`, `src/distributed.rs`, `src/store.rs`, and `src/postgres_store.rs` implement two durable engines and fenced worker claims. Existing specifications promise acyclic process graphs and only four built-in types; their deltas are explicit in this change.

## Goals / Non-Goals

**Goals:** Extend the compiled language and both engines without a second rules interpreter or queue. Preserve execution and serialized status behavior for existing acyclic processes; make repeated work, waits, and deadlines recoverable.

**Non-Goals:** FEEL parsing, external-signal waits, BPMN/DMN XML interchange, new public REST endpoints, or guaranteed exactly-once external side effects.

## Decisions

### Compiled decision graphs, not an external engine

Introduce a `decision` model declaration with typed input/output and named `decision` and `knowledge` elements. Decisions choose exactly one expression, table, or context implementation; edges name dependencies and are checked with a topological pass. Knowledge models are typed parameterized pure functions; contexts are ordered named bindings ending in a result expression. Decision tables use the existing expression parser extended for typed DateTime instants and table rule predicates. The business-rule process node compiles to an evaluator closure producing a typed value, just like task nodes. Reject cycles inside decision graphs and knowledge calls even though process graphs can cycle. This reuses compiler and generated-code conventions; translating models to anonymous tasks would hide policy semantics, while a separate DMN runtime adds a second execution/serialization boundary.

Implement the seven hit policies in one table evaluator with explicit ordered rules; normalize outputs to a typed scalar/record for one result or a list for multiple. Priorities require a declared rank for each output value that actually matches; a missing rank fails evaluation. ANY compares full multi-column outputs. SUM/MIN/MAX require one numeric output; COUNT returns `Number`. No-match defaults are typed to the table's policy result and override the natural empty result, if any. Do not introduce null or FEEL; unsupported predicate syntax receives a compile diagnostic. Supply compiler and generated-code tests for each policy, aggregation, multiple outputs, priority ties/unranked outputs, no match, and invalid dependency graphs.

### Typed waits and deadlines

Add `DateTime` as an offset-aware RFC 3339 serialized instant; compile `pause until` against typed `.bl` expressions. Reuse the duration grammar already used by retry declarations for `pause for`, task time bounds, and process deadlines. At node entry calculate and persist the wake instant once, then transition to `waiting`; workers release their claims/permits. A past timestamp immediately advances. Source spelling is `node hold = pause_for "1m"` or `node hold = pause_until input.closes_at`, with a process-level `deadline queued "10m"` or `deadline first_claimed "10m"` before nodes. A local poller and the distributed claim/reconciliation loops make eligible waits runnable; waiting does not consume an attempt or reset its retry state. `GET /instances/{id}` retains existing fields and adds status `waiting`, `wake_at`, and deadline/first-claim timestamps where applicable; cancellation works during waits. This is preferable to sleeping in a worker, which would waste capacity and lose wake semantics on takeover.

Persist creation time at millisecond precision for queue-origin deadlines and a `first_claim_at_ms` assigned atomically with the first successful claim, never overwritten. A configured deadline has exactly one origin (`queued` or `first_claimed`) and a positive duration. Reconciliation checks queued, waiting, retry-waiting, and running instances; first-claim policy has no deadline until its first claim. Timeout transitions are atomic with completion/cancellation and use the existing business-error status with reserved terminal name `timeout` (no synthetic user-authored edge); task-loop limit uses reserved `task-iteration-limit`. After terminalization, reject late fenced writes and signal in-flight hooks. Unlike ordinary execution errors, these outcomes are not retried.

### Activation-scoped cyclic execution

Replace node-name-as-instance checkpoint identity with activation IDs and scoped result maps. A token carries its generation/route, including split invocation identity; joins match active branches of the same split invocation. Re-entering a node creates a new activation; a completed activation stays committed and is not replayed. Conditions reference the most recent result on the current token's route; values at merges must be definitely available and unambiguous on all incoming routes. Replace DAG scope construction with a fixpoint/data-flow analysis for cyclic regions and reject ambiguous cross-branch or uninitialized references. Validate backward links for legal split/join structure, reachable exits, and mandatory process deadline. This avoids the unsafe alternative of clearing the whole checkpoint on each cycle.

A task loop is one node with explicit pre-check or post-check condition, persisted iteration state, and a required positive count and/or elapsed-time bound; source syntax is `node repeated = task echo(repeated) repeat_pre(repeated < 3) max_iterations 3 initial input` or `node repeated = task echo(input) repeat_post(repeated < 3) max_duration "1m"`. Both bounds may be combined; the pre-check zero-iteration case requires a typed initial result. Multi-instance syntax is `node batch = task echo each input parallel` or `sequential`, where `input` is a typed list. A multi-instance task iterates a finite typed list with one activation per item, sequentially or in parallel under the existing concurrency limit, persisting item-indexed outputs and returning them in source order. Empty list yields empty result. Reaching a bound emits the reserved business error; elapsed-time bounds and process deadlines are cooperative for in-flight synchronous calls, but late results cannot commit after terminalization. Retries rerun only uncommitted activations. Keep graph cycles and task loop state separate so a repeated node can itself contain an independently bounded loop.

## Risks / Trade-offs

- [Cyclic parallel graphs can create ambiguous merges or unbounded activation growth] → Validate joins per split generation and references per route; require a process deadline for cycles; test branching back-edges and fail ambiguous graphs at compile time.
- [Restart/worker loss near a wake or deadline can race with completion] → Persist wake/deadline and use the existing serialized transitions, owner/generation fencing, and restart reconciliation in both stores.
- [A synchronous task cannot be preempted] → Signal cancellation, suppress late results, and retain documented at-least-once effects.
- [Broad table policy behavior is easy to misimplement] → Table-driven tests for all seven policies, four aggregations, multiple outputs, defaults, and invalid combinations against the explicit policy contract.

## Migration Plan

Add additive columns and checkpoint versioning to local and PostgreSQL stores. Decode old acyclic checkpoints into the new activation representation or keep a versioned decoder for them; do not discard active instances or silently replay committed tasks. Existing source compiles unchanged; roll out API and matching worker binaries together for new model versions, while old-version workers drain their own backlog. Rollback to an old binary is safe only before new-format instances/checkpoints are created; otherwise keep upgraded workers until that backlog finishes.
