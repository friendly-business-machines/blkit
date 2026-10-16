# Design

## Context

See `proposal.md` and `specs/distributed-execution/spec.md`. `DistributedWorker::run_once` claims up to `limit` instances and uses a `JoinSet` to execute them, but does not call `claim` again until the whole batch completes. It renews leases for the active set on a timer. Generated and harness workers call `run_once` in a 100 ms outer loop and then check for draining. `PostgresStore` clones share one `Arc<Mutex<Client>>`; combined API+worker binaries also share it with a 100 ms reconciler. The database claim query already checks the worker's draining state. Leases, checkpoint fencing, exact identity routing, and existing recovery behavior must remain intact.

## Goals / Non-Goals

**Goals:**
- Reuse free instance slots promptly despite an unrelated long-running claim, while keeping no more than `limit` active instances and retaining the existing semaphore's task-level limit.
- Preserve active-claim heartbeat/renewal, external drain behavior, and `run_once`'s total-claims return value for callers/tests.
- Produce reproducible lock-contention measurements and make an explicit keep-or-change decision, based on measured benefit.

**Non-Goals:**
- Change queue schema, claim fencing, retry semantics, public CLI flags, or the local persistence worker.
- Add a pool or connection-management framework without evidence; tune unrelated SQL/indexes.

## Decisions

1. **Refill within the existing worker loop.** Keep a single active map and `JoinSet` across claims. After a completion, remove the finished claim and request only `limit - active.len()` additional instances. While active and below capacity, poll for newly queued eligible work at the existing 100 ms cadence; return promptly when active reaches zero and no work is available so the outer loop can perform its normal drain/idle handling. Renew all remaining claims on the lease timer even during idle-slot polling. Count all claims made during the call in its return value. Reuse the current database claim/drain gate and semaphore instead of introducing a second scheduler or an unbounded spawn loop. Alternative: simply shrink batches or claim again only after a completion; the latter leaves slots idle if eligible work arrives after an earlier completion.

2. **Honor drain at each refill, not just at the end of the outer loop.** The existing `claim` query excludes draining workers, so refills after a committed drain find no eligible work while owned work continues to renew and finish. Test drain during an active partial batch with remaining backlog and concurrent competing workers. Keep check/claim transaction ordering safe at the store boundary if a race between drain and refill is observed; do not bypass the database gate with a cached worker state. Alternative: rely on `drain_if_requested` only after `run_once`; it cannot run until the active set empties.

3. **Measure connection lock contention before changing connections.** Under a fixed PostgreSQL test setup, capture baseline and post-refill behavior for (a) an API-only process with concurrent starts/status/cancels and reconciliation and (b) a combined worker/API process with `limit=32`, queued short and long tasks, checkpoints, lease renewals, and API requests. Record workload, database/runtime environment, repeated-run throughput, p50/p95 operation latency, p50/p95 time waiting to acquire the existing mutex, and time spent doing database work. Use temporary targeted timing at existing lock sites in the harness or a diagnostic build; do not add always-on logging of inputs or credentials. Record the commands, results, and decision in `openspec/changes/keep-distributed-workers-busy-and-measure-db-contention/measurement.md`. If lock waits are material relative to operation latency and a small bounded multi-connection prototype improves p95 latency/throughput without breaking correctness, choose the smallest bounded approach: separate already-supported connections for combined API and worker when contention is mostly cross-role, otherwise a small established pool for a single busy role. Keep the single connection if either condition is absent; no new dependency just for an unproven speedup. Re-run the same workload and concurrency/fencing tests if connections change. Alternative: add a pool first; it adds connection pressure and complexity before identifying a bottleneck.

## Risks / Trade-offs

- [Fast refills can increase database polling] -> Poll only with active work and spare slots, at the existing worker cadence; retain idle outer-loop behavior.
- [Long-running `run_once` may postpone `drain_if_requested`] -> Database-backed claim gating blocks new claims after drain; permit active work to finish and then return. Cover continuous backlog in the drain test.
- [Concurrent refill, renewal, cancellation, or drain can race] -> Keep single ownership of the active set, preserve generation fencing and database claim predicates, and exercise races in PostgreSQL tests.
- [Benchmark results depend on the host and workload] -> Record the environment and repeat comparable before/after runs; do not assert universal speedups from synthetic results.
- [Additional connections can raise PostgreSQL connection usage or change transaction serialization] -> Make the change only with measured benefit; bound connections, keep per-transaction ownership, and re-run multi-worker fencing tests.

## Migration Plan

No data migration. Existing generated binaries pick up worker behavior when rebuilt against the updated core library. If measurement justifies a connection change, keep `PostgresStore::connect` and generated CLI arguments compatible. Roll back by rebuilding with the earlier core version; in-flight claims remain fenced by their existing lease/generation fields.
