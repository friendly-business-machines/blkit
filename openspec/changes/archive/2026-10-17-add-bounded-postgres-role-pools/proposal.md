# Proposal

## Why

Separating the API and worker connections removed cross-role contention, but each role still serializes its database operations through a single `PostgresStore` client. A single high-capacity API-only, worker-only, or combined node can therefore hit a client-side ceiling even while PostgreSQL has capacity; CPU count alone is not a safe connection budget.

## What Changes

- Replace the single shared PostgreSQL client per store with a bounded, lazy pool using `deadpool-postgres`; keep `PostgresStore::connect(url)` source-compatible and expose a pool-size constructor for library callers.
- Give generated remote API and worker roles independently configurable connection limits with conservative defaults, including separate budgets in a combined binary. Keep positional CLI arguments and local-persistence targets unchanged.
- Preserve startup database/schema checks, transaction-scoped connection ownership, bounded checkout waits, claim/drain ordering, leases and generation fencing; report invalid configuration or unavailable connections rather than silently overcommitting.
- Measure single-client and multiple bounded pool sizes in API-only, worker-only, and combined workloads, including connection counts, throughput, latency, and contention; document the results, selected default, and deployment-wide PostgreSQL connection budgeting without promising that more connections always improve performance.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `distributed-execution`: Specify bounded, independently budgeted PostgreSQL connections for remote API and worker roles, including configuration, failure behavior, and preservation of distributed claim/lease safety.

## Impact

- `crates/blkit-core/src/postgres_store.rs`, the optional `remote-persistence` dependency in `crates/blkit-core/Cargo.toml`, and `Cargo.lock`.
- Remote generated entrypoints in `crates/blkit-transpiler/src/project.rs`, optionally the development-harness binaries, README deployment guidance, and their PostgreSQL/generated-project tests and performance harness.
- Existing Rust callers retain the `PostgresStore::connect` signature but may use more PostgreSQL sessions under load; operators must budget the sum of per-process role limits against server/proxy connection limits. No schema or payload migration.
