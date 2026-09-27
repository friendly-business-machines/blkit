# Proposal

## Why

The current single-node runtime executes a compiled `.bl` graph inside one server and marks interrupted instances failed on restart. It cannot route versioned instances across worker processes or resume unfinished work without repeating completed tasks. Distributed execution needs durable ownership and process checkpoints while keeping `.bl` the source of graph and retry behavior.

## What Changes

- Add PostgreSQL-backed distributed instance records, queue/claims, worker capability registration, heartbeats, leases, version-aware routing, and graceful draining. Each instance has at most one owner; each worker binary can run many instances concurrently. No separate message broker.
- Checkpoint task outputs and process routing state; on execution errors or worker loss, resume unfinished work according to a source-declared retry policy. Preserve completed parallel tasks. Apply the same retry/checkpoint semantics in the local Turso dev server.
- **BREAKING:** Replace implicit process sequencing and process-level `return` with named graph nodes and explicit links, including named `end`, `error`, `cancel`, and `terminate` terminal nodes. Task bodies retain `return` for their outputs; process graphs remain acyclic.
- Distinguish modeled business-error termination from retryable execution failure in persisted status and REST responses. Keep the existing start/status/cancel REST control surface.
- Defer loops, scoped BPMN catch/transaction events, external task languages, client interaction, and detection of incompatible compiled graphs published under one process identity.

## Capabilities

### New Capabilities

- `distributed-execution`: PostgreSQL-backed multi-worker ownership, capability/version registration, queueing, leases, checkpoints, retry, and drain behavior.

### Modified Capabilities

- `business-language`: Explicitly linked named graph and terminal nodes; source-declared retry policy; remove process-level `return` while retaining task `return`.
- `process-runtime`: Checkpointed recovery and retries in local mode; distinguish terminal outcomes and execution failures; preserve REST cancellation and status across modes.

## Impact

- Changes the `.bl` parser, semantic checks, generated graph, example files, tests, and README; existing `.bl` process sources require migration.
- Changes graph execution, instance state/storage, and server recovery; adds a PostgreSQL-backed distributed server/worker path while retaining local Turso development mode.
- Adds a PostgreSQL client dependency and a PostgreSQL service for distributed deployments and integration tests; no broker or runtime `.bl` compiler is required. Generated Rust is linked into worker binaries at build time.
