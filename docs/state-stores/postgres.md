# PostgreSQL

> A durable, strongly consistent, shareable state store backed by PostgreSQL — a
> good default for production.

The PostgreSQL backend keeps each run's state in a PostgreSQL database. It is
**durable** (the data survives restarts), **strongly consistent** (a write is
always visible to the next read), and **shareable** (workers on different machines
can all reach the same database), which makes it a good default for production.

It lives in its own module, so the database driver it needs is only pulled in by
applications that use it:

```go
import (
    "os"

    bl      "github.com/friendly-business-machines/blkit"
    pgstore "github.com/friendly-business-machines/blkit/stores/postgres"
)

// The DSN points at the server and database; the same string handed to workers
// in other processes or on other machines lets them all work on the same runs.
var store = pgstore.New(pgstore.Config{DSN: os.Getenv("DATABASE_URL")})
```

The backend is built on [pgx](https://github.com/jackc/pgx), the actively
maintained PostgreSQL driver for Go: a whole batch of writes is sent as one
pipelined round-trip inside a single transaction, and encoded values are stored as
`JSONB` so the audit history is queryable with SQL down into the values.

## What it's good for

- **Production runs** that must survive restarts and crashes.
- **Runs shared across many workers**, on one machine or many.
- **Auditing and reporting** — because the history is ordinary database rows, it
  can be queried directly with SQL.

## Data model

Everything blkit persists about a run maps onto **three tables**, created on first
use. Their names carry a configurable prefix (`blkit_` by default, see
[Configuration](#configuration)), so the schema shown here is `blkit_runs`,
`blkit_values`, and `blkit_history`.

### `blkit_runs` — one row per run

The run's identity and lifecycle: which process it is an instance of, where it has
got to, and when.

| Column | Type | Purpose |
|---|---|---|
| `run_id` | `TEXT PRIMARY KEY` | The run's id — the key every other table joins on. |
| `process_id` | `TEXT` | The process this run is an instance of. |
| `process_version` | `TEXT` | The version of that process definition. |
| `status` | `TEXT` | Lifecycle phase of the run. |
| `published_at` / `started_at` / `completed_at` | `TIMESTAMPTZ` | Lifecycle timestamps (nullable until each is reached). |
| `evaluation_count` | `INTEGER` | How many times the run has been advanced — the optimistic-progress counter. |

**Example row**, for a run of an `order-approval` process that has finished:

| run_id | process_id | process_version | status | published_at | started_at | completed_at | evaluation_count |
|---|---|---|---|---|---|---|---|
| `run_8f2c1a90` | `order-approval` | `v1` | `completed` | `2026-07-08 09:11:59.900+00` | `2026-07-08 09:12:00.000+00` | `2026-07-08 09:12:00.820+00` | `3` |

### `blkit_values` — one row per value a task writes

This is the heart of the model. blkit never overwrites a field; **every write is a
new row**, and the current value of a field is *derived* from these rows rather
than stored in place.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGSERIAL PRIMARY KEY` | Monotonic arrival order — the tie-breaker for two rows with equal timestamps. |
| `run_id` | `TEXT` | The run this write belongs to. |
| `task_id` | `TEXT` | The task that produced it. |
| `execution_id` | `TEXT` | The specific execution attempt of that task (a retried task runs under a new execution id). |
| `field` | `TEXT` | The field path being written. |
| `value` | `JSONB` | The encoded value — `JSONB`, so audit queries can reach inside it. |
| `status` | `TEXT` | `pending`, `committed`, or `aborted` (see below). |
| `ts` | `TIMESTAMPTZ` | When the write was made. |

**Example rows**, continuing the same run — `check-inventory` succeeded on its
first attempt, but `approve-order` failed once (`exec_b1`) and committed on retry
(`exec_b2`):

| id | run_id | task_id | execution_id | field | value | status | ts |
|---|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `check-inventory` | `exec_a1` | `in_stock` | `true` | `committed` | `2026-07-08 09:12:00.340+00` |
| `2` | `run_8f2c1a90` | `approve-order` | `exec_b1` | `approved` | `true` | `aborted` | `2026-07-08 09:12:00.610+00` |
| `3` | `run_8f2c1a90` | `approve-order` | `exec_b2` | `approved` | `true` | `committed` | `2026-07-08 09:12:00.780+00` |

Row 2 is the failed attempt's write — it stays in the table but never satisfies a
current-state read. The current state for this run is therefore `in_stock: true`
and `approved: true`, resolved from rows 1 and 3 only.

Two behaviours defined by the [shared conformance suite](overview.md#conformance)
are implemented entirely through the `status` column:

- **A task's outputs become visible all at once, when it finishes.** While a task
  runs, each value it writes lands as a `pending` row and is invisible to the
  current state. When the task completes, all of its pending rows flip to
  `committed` in a **single statement**; if it fails, they flip to `aborted`. The
  outputs never appear half-written.
- **Nothing is ever deleted.** Aborted rows stay in the table for diagnostics and
  audit — they simply never satisfy the current-state read.

Two reads sit on top of these rows:

- **Current state** — the latest `committed` row per `field`, resolved with a
  `ROW_NUMBER()` window partitioned by `field` and ordered by `(ts, id)` descending.
  The newest committed write wins.
- **Full history** — every row for the run, each carrying its `status`, so a failed
  attempt is visible as an aborted write next to the committed one that superseded it.

Two indexes keep both reads cheap: a **replay** index on `(run_id, ts, id)` and a
**partial** index on `(run_id, task_id) WHERE status = 'pending'` so committing a
task's writes touches only its own pending rows.

### `blkit_history` — one row per execution-history entry

The execution history — the record of what the engine did, step by step.

| Column | Type | Purpose |
|---|---|---|
| `id` | `BIGSERIAL PRIMARY KEY` | Arrival-order tie-breaker, as in `blkit_values`. |
| `run_id` | `TEXT` | The run this entry belongs to. |
| `kind` | `TEXT` | The kind of history entry. |
| `node_id` | `TEXT` | The graph node it concerns (nullable). |
| `execution_id` | `TEXT` | The execution attempt it belongs to. |
| `payload` | `JSONB` | The entry's detail. |
| `ts` | `TIMESTAMPTZ` | When it was recorded. |

**Example rows**, for the same run:

| id | run_id | kind | node_id | execution_id | payload | ts |
|---|---|---|---|---|---|---|
| `1` | `run_8f2c1a90` | `task_started` | `check-inventory` | `exec_a1` | `{}` | `2026-07-08 09:12:00.120+00` |
| `2` | `run_8f2c1a90` | `task_completed` | `check-inventory` | `exec_a1` | `{}` | `2026-07-08 09:12:00.410+00` |
| `3` | `run_8f2c1a90` | `task_failed` | `approve-order` | `exec_b1` | `{"error": "validation timeout"}` | `2026-07-08 09:12:00.640+00` |
| `4` | `run_8f2c1a90` | `task_completed` | `approve-order` | `exec_b2` | `{}` | `2026-07-08 09:12:00.820+00` |

Unlike the current-state read over `blkit_values`, this table keeps row 3's failed
attempt visible right alongside the retry that superseded it — nothing here is
ever discarded.

Like the values table it is append-only and read back sorted on `(ts, id)`.

### Ordering under parallelism

Both event tables carry a timestamp **and** an auto-increment id, and every read
sorts on `(ts, id)`. Parallel tasks within a run each append their own rows
concurrently; the arrival order they happen to hit the database in does not matter,
because the stored `(ts, id)` pair — not insertion order — defines replay order.
This is what makes the history stable and reproducible across backends.

## Configuration

```go
type Config struct {
    DSN         string // PostgreSQL connection string; required
    TablePrefix string // table-name prefix; defaults to "blkit_"
}
```

- **`DSN`** — the connection string for the server and database (for example
  `postgres://<username>:<password>@<host>:5432/blkit`). Because the data lives in a shared
  database, the same `DSN` handed to workers in other processes or on other
  machines lets them all work on the same runs.
- **`TablePrefix`** — the prefix on the three table names. Change it to run more
  than one independent blkit deployment inside a single database without their
  tables colliding.

### Managed and cloud PostgreSQL

This backend speaks to any PostgreSQL-compatible server, so it also works — with no
code change, just a different `DSN` — against the managed PostgreSQL services on the
major clouds:

- **AWS** — Amazon RDS for PostgreSQL, and Amazon Aurora PostgreSQL-Compatible Edition.
- **GCP** — Cloud SQL for PostgreSQL, and AlloyDB for PostgreSQL.
- **Azure** — Azure Database for PostgreSQL.

There is no separate backend for these — point the `DSN` at the managed endpoint.
Use a primary (read-write) endpoint; reading from an asynchronous read replica can
return stale data.

## Consistency

PostgreSQL is strongly consistent against a single primary: a committed write is
visible to the next read, so a worker always sees its own prior writes and those of
any other worker that finished first. The one caveat is **read replicas** — an
asynchronous replica can lag the primary, so blkit must always be pointed at the
primary (read-write) endpoint, never a replica.

## What to keep in mind

- **Point at the primary, not a replica** — see Consistency above.
- **The backend creates its own schema** on first use, so the connecting role needs
  `CREATE` privilege on the target database the first time it runs; afterwards only
  `SELECT`/`INSERT`/`UPDATE` on the three tables are used.
- **History grows without bound** — nothing is deleted by design. For long-lived
  deployments, plan a retention or archival policy against the tables directly.

## Concurrency

Different runs are independent rows keyed by different run ids, so many runs — and
many workers — can share one database without interfering. Parallel tasks within a
run each write their own rows, and the order is sorted out from the timestamps at
read time.

## Reference

The backend's API is in the [PostgreSQL reference](../reference/stores-postgres.md);
the `StateStore` interface it implements is in the core [Reference](../reference/blkit.md).
