# Proposal

## Why

`.bl` currently supports only acyclic, immediately executing process graphs and simple task expressions. Decision-heavy business rules and long-running, repeating workflows (such as pricing, approvals, and fulfillment) cannot be modeled directly or resumed safely.

## What Changes

- Add typed decision models invoked by business-rule process nodes, with acyclic decision requirement graphs, literal expressions, boxed contexts, reusable business knowledge models, and full DMN hit-policy/aggregation behavior for decision tables using `.bl` expressions (not FEEL).
- Add `DateTime` expressions and process intermediate nodes `pause for` (duration) and `pause until` (a `DateTime` expression); resume from durable wake times without holding a worker claim.
- Support task loops with mandatory iteration/time bounds, sequential and parallel multi-instance tasks over lists, and process back-edges. Require a process deadline for graphs containing back-edges.
- Add process deadlines selectable from queue time or first claim, and process-wide timeout as a named business-error outcome that stops all active work. Support persisted repeated activations and safe recovery in both local and distributed modes.
- **BREAKING**: The existing contract that process graphs must be acyclic, and the fixed list of built-in types, change. Existing acyclic `.bl` sources remain valid.
- External-signal/input waits and FEEL compatibility are out of scope.

## Capabilities

### New Capabilities
- `decision-models`: Typed `.bl` decision graphs and decision-table evaluation semantics.

### Modified Capabilities
- `business-language`: Add `DateTime`, decision calls, waits, bounded iteration, and validated cyclic process graphs.
- `process-runtime`: Add durable wake scheduling, activation-scoped iteration, deadlines, and timeout outcomes in the local engine and shared execution semantics.
- `distributed-execution`: Persist and reconcile deadlines/waits and repeated activations under fenced claims, including unclaimed queued instances.

## Impact

Touches `.bl` parsing, typing, graph validation, Rust generation, runtime checkpoints/scheduling, Turso/PostgreSQL storage and migrations, REST instance status, examples, and compiler/runtime/integration tests. Compiled worker binaries must be rebuilt for new definitions; no external DMN engine or broker is introduced.
