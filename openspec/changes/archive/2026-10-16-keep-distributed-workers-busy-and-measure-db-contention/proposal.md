# Proposal

## Why

A distributed worker currently waits for its entire claimed batch to finish before requesting more work. One slow instance can leave freed execution capacity idle despite eligible backlog. At the same time, the PostgreSQL store serializes operations through one connection and mutex, but there is no evidence yet that connection contention warrants a pool.

## What Changes

- Refill available worker capacity while other claimed instances are still running, without exceeding the configured task limit or claiming after a drain request.
- Measure time spent waiting for the PostgreSQL store's client lock under representative concurrent API/worker load; record workload, baseline and results before deciding whether to change connection management.
- Only if measurements demonstrate material lock contention and a bounded multi-connection approach improves it, implement and verify that approach; otherwise retain the existing single connection. No unconditional pooling dependency or user-facing connection option.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `distributed-execution`: Require distributed workers to reuse freed capacity while active, subject to claim ownership, limits, and graceful drain semantics.

## Impact

- Worker scheduling in `crates/blkit-core/src/distributed.rs`, with PostgreSQL-backed concurrency/drain tests in `tools/dev-harness/tests/postgres.rs`.
- PostgreSQL store (`crates/blkit-core/src/postgres_store.rs`) and potentially generated remote binaries in `crates/blkit-transpiler/src/project.rs` and harness entrypoints if measurements justify a bounded connection change. Any optional dependency change is conditional on measured benefit. Existing store APIs, fencing, admission, and on-disk state remain compatible.
