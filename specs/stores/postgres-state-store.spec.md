---
name: PostgresStateStore
description: A durable state-store backend that keeps each run's ProcessState in PostgreSQL — its own module, so only applications that use it pull in the database driver
targets:
  - ../../stores/postgres/store.go
---

# PostgresStateStore

> **Status:** Work in progress. See
> [overview.spec.md](./overview.spec.md) for how backends are laid out, and
> [process-state.spec.md](../processes/process-state.spec.md) for what a
> `ProcessState` is.

The PostgreSQL backend keeps each run's
[ProcessState](../processes/process-state.spec.md) in a PostgreSQL database. It is
**durable** (the data survives restarts) and **shareable** (workers on different
machines can all reach the same database), which makes it a good default for
production.

It lives in **its own module**, `github.com/friendly-business-machines/blkit/stores/postgres`,
so the database driver it needs is only pulled in by applications that actually use
it.

```go
import (
    "os"

    bl      "github.com/friendly-business-machines/blkit"
    pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

var store = pgstore.New(pgstore.Config{DSN: os.Getenv("DATABASE_URL")})
```

---

## Implementation

The backend is built on **`github.com/jackc/pgx/v5`** — the canonical, actively
maintained PostgreSQL driver for Go — using a `pgxpool.Pool` for connection pooling.
pgx is chosen over the generic `database/sql` + `lib/pq` route for two
PostgreSQL-specific wins:

- **`pgx.Batch`** — a whole `WriteBatch` of ops is sent as one pipelined round-trip
  inside a single transaction, which is exactly the shape the worker's writer pool
  produces.
- **Native `JSONB` and `TIMESTAMPTZ` codecs** — encoded `Bl` values are stored as
  `JSONB`, so the audit history is queryable with SQL down into the values.

## Storage layout

Three tables, created by the backend on first use (a configurable prefix defaults to
`blkit_`), implementing the [write contract](./overview.spec.md#the-write-contract):

```sql
CREATE TABLE blkit_runs (
    run_id           TEXT PRIMARY KEY,
    process_id       TEXT NOT NULL,
    process_version  TEXT NOT NULL,
    status           TEXT NOT NULL,          -- run status, written via Save
    published_at     TIMESTAMPTZ,
    started_at       TIMESTAMPTZ,
    completed_at     TIMESTAMPTZ,
    evaluation_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE blkit_values (
    id           BIGSERIAL PRIMARY KEY,      -- arrival order (replay tiebreak)
    run_id       TEXT NOT NULL,
    task_id      TEXT NOT NULL,
    execution_id TEXT NOT NULL,
    field        TEXT NOT NULL,
    value        JSONB NOT NULL,             -- encoded Bl value
    status       TEXT NOT NULL DEFAULT 'pending', -- pending | committed | aborted
    ts           TIMESTAMPTZ NOT NULL
);
CREATE INDEX blkit_values_replay  ON blkit_values (run_id, ts, id);
CREATE INDEX blkit_values_pending ON blkit_values (run_id, task_id)
    WHERE status = 'pending';

CREATE TABLE blkit_history (
    id           BIGSERIAL PRIMARY KEY,      -- arrival order (replay tiebreak)
    run_id       TEXT NOT NULL,
    kind         TEXT NOT NULL,              -- e.g. NODE_STARTED
    node_id      TEXT,
    execution_id TEXT NOT NULL,
    payload      JSONB NOT NULL,             -- remaining entry fields
    ts           TIMESTAMPTZ NOT NULL
);
CREATE INDEX blkit_history_replay ON blkit_history (run_id, ts, id);
```

How the write ops map onto it:

- **`ValueWrite`** → `INSERT` into `blkit_values` with `status = 'pending'`.
- **`StatusFlip`** →
  `UPDATE blkit_values SET status = $3 WHERE run_id = $1 AND task_id = $2 AND status = 'pending'`
  — one statement settles all of the task's pending rows atomically (the partial
  index above makes it cheap).
- **`HistoryEntry`** → `INSERT` into `blkit_history`.
- A `WriteBatch` executes as a single transaction via `pgx.Batch`; `Flush` is a
  no-op beyond confirming the transaction committed, because writes are synchronous
  once the batch returns.
- **Current version** — latest committed value per field:
  `SELECT DISTINCT ON (task_id, field) ... WHERE run_id = $1 AND status = 'committed' ORDER BY task_id, field, ts DESC, id DESC`.
- **Full history** — all rows for the run from both tables, merged and sorted by
  `(ts, id)` (see [Ordering and replay](./overview.spec.md#ordering-and-replay)).
  Aborted and pending rows are included, with their status.

---

## Configuration

The backend is constructed with the connection details for the database — most
importantly a connection string (`DSN`) pointing at the PostgreSQL server and
database to use.

Because the data is in a shared database, these same connection details can be
handed to workers running in other processes or on other machines, so they can all
work on the same runs.

### Managed and cloud PostgreSQL

This backend speaks to any PostgreSQL-compatible server, so it also works — with no
code change, just a different `DSN` — against the popular managed PostgreSQL services
on the major clouds:

- **AWS** — Amazon RDS for PostgreSQL, and Amazon Aurora PostgreSQL-Compatible Edition.
- **GCP** — Cloud SQL for PostgreSQL, and AlloyDB for PostgreSQL.
- **Azure** — Azure Database for PostgreSQL.

There is no separate backend for these — point this backend's `DSN` at the managed
endpoint. Use a primary (read-write) endpoint; reading from an asynchronous read
replica can return stale data.

---

## What it is good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Auditing and reporting** — because the history is stored as ordinary database
  rows, it can be queried directly with SQL.

---

## Concurrency

- **Different runs** are completely independent rows keyed by different run ids, so
  many runs — and many workers — can use the same database at once without
  interfering.
- **Parallel tasks within one run** each write their own rows, so their writes do
  not overwrite each other. The order is sorted out from the timestamps at read time.

---

## Testing

This backend is verified against the shared state-store **conformance suite** (see
[overview.spec.md](./overview.spec.md#testing)). The suite runs against a **real
PostgreSQL server**: the test reads the server address from the `BLKIT_TEST_POSTGRES_DSN`
environment variable and skips when it is unset. CI provides the server in a
container; locally, point the variable at any disposable instance. Each subtest
uses its own table prefix, so runs are isolated and repeatable, and the tables are
dropped afterwards.

`[@test] ../../stores/postgres/store_test.go`
